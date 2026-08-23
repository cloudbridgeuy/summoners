//! Cache file-system operations with platform-specific safety boundaries.

#[cfg(unix)]
mod imp {
    use std::ffi::{CStr, CString};
    use std::fs;
    use std::io::{self, Read, Write};
    use std::os::fd::OwnedFd;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    use rustix::fs::{
        AtFlags, Dir, FileType, Mode, OFlags, fstat, mkdirat, open, openat, renameat, statat,
        unlinkat,
    };
    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    use rustix::fs::{RenameFlags, renameat_with};

    use super::super::{CacheLayer, CacheLayerStats, Freshness, classify_entry};

    static FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
    static TOMBSTONE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const READ_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::NOFOLLOW)
        .union(OFlags::NONBLOCK)
        .union(OFlags::CLOEXEC);

    #[derive(Debug)]
    pub(crate) struct Snapshot {
        parent: OwnedFd,
        name: String,
        identity: Identity,
        directory: Option<OwnedFd>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Identity {
        device: u128,
        inode: u128,
        file_type: FileType,
    }

    struct RemovalPlan {
        children: Vec<PinnedEntry>,
        files: usize,
    }

    struct PinnedEntry {
        name: CString,
        identity: Identity,
        directory: Option<PinnedDirectory>,
    }

    struct PinnedDirectory {
        handle: OwnedFd,
        children: Vec<PinnedEntry>,
        files: usize,
    }

    pub(crate) fn read(root: &Path, directories: &[&str], name: &str) -> io::Result<Vec<u8>> {
        let root_fd = open_directory(root)?;
        let directory = open_directories(&root_fd, directories, false)?;
        read_regular_at(&directory, name).map(|(bytes, _)| bytes)
    }

    pub(crate) fn write(
        root: &Path,
        directories: &[&str],
        name: &str,
        bytes: &[u8],
    ) -> io::Result<()> {
        let root_fd = open_or_create_root(root)?;
        let directory = open_directories(&root_fd, directories, true)?;
        let sequence = FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = format!(".entry-{}-{sequence}.tmp", std::process::id());
        let file_fd = openat(
            &directory,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(io::Error::from)?;
        let mut file = fs::File::from(file_fd);
        let result = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = result {
            let _ = unlinkat(&directory, temporary.as_str(), AtFlags::empty());
            return Err(error);
        }
        if let Err(error) = renameat(&directory, temporary.as_str(), &directory, name) {
            let _ = unlinkat(&directory, temporary.as_str(), AtFlags::empty());
            return Err(error.into());
        }
        Ok(())
    }

    pub(crate) fn remove(root: &Path, directories: &[&str], name: &str) -> io::Result<()> {
        let root_fd = open_directory(root)?;
        let directory = open_directories(&root_fd, directories, false)?;
        unlinkat(&directory, name, AtFlags::empty()).map_err(io::Error::from)
    }

    pub(crate) fn collect_stats(
        root: &Path,
        layer_name: &str,
        now: SystemTime,
        layer: CacheLayer,
    ) -> CacheLayerStats {
        let Ok(root_fd) = open_directory(root) else {
            return CacheLayerStats::default();
        };
        let Ok(layer_fd) = open_directory_at(&root_fd, layer_name) else {
            return CacheLayerStats::default();
        };
        let mut stats = CacheLayerStats::default();
        visit_best_effort(&layer_fd, &mut |directory, name, file_type, size| {
            let freshness = if file_type == FileType::RegularFile {
                read_regular_at(directory, name)
                    .ok()
                    .and_then(|(bytes, _)| classify_entry(&bytes, now, layer))
                    .unwrap_or(Freshness::Expired)
            } else {
                Freshness::Expired
            };
            match freshness {
                Freshness::Fresh => stats.fresh.add_file(size),
                Freshness::Expired => stats.expired.add_file(size),
            }
        });
        stats
    }

    pub(crate) fn prune(
        root: &Path,
        layer_name: &str,
        now: SystemTime,
        layer: CacheLayer,
    ) -> usize {
        let Ok(root_fd) = open_directory(root) else {
            return 0;
        };
        let Ok(layer_fd) = open_directory_at(&root_fd, layer_name) else {
            return 0;
        };
        prune_directory(&layer_fd, now, layer, &mut |_| {})
    }

    pub(crate) fn detach_root(root: &Path) -> io::Result<Option<Snapshot>> {
        detach_root_with_before_rename(root, || {})
    }

    fn detach_root_with_before_rename(
        root: &Path,
        before_rename: impl FnOnce(),
    ) -> io::Result<Option<Snapshot>> {
        let (parent_path, root_name) = split_root(root)?;
        let parent = match open(&parent_path, DIRECTORY_FLAGS, Mode::empty()) {
            Ok(parent) => parent,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let root_metadata = match statat(&parent, &root_name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => metadata,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let identity = identity_from_stat(&root_metadata);
        let directory = if identity.file_type == FileType::Directory {
            let handle = openat(&parent, &root_name, DIRECTORY_FLAGS, Mode::empty())
                .map_err(io::Error::from)?;
            verify_handle_identity(&handle, identity)?;
            Some(handle)
        } else {
            None
        };
        before_rename();

        loop {
            let sequence = TOMBSTONE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let tombstone = format!(".cceroby-clear-{}-{sequence}", std::process::id());
            match rename_noreplace(&parent, &root_name, &tombstone) {
                Ok(()) => {
                    verify_identity(&parent, tombstone.as_str(), identity)?;
                    return Ok(Some(Snapshot {
                        parent,
                        name: tombstone,
                        identity,
                        directory,
                    }));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    return Err(snapshot_changed());
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
    }

    pub(crate) fn remove_snapshot(snapshot: &Snapshot) -> io::Result<usize> {
        remove_snapshot_with_before_delete(snapshot, || {})
    }

    fn remove_snapshot_with_before_delete(
        snapshot: &Snapshot,
        before_delete: impl FnOnce(),
    ) -> io::Result<usize> {
        let plan = pin_snapshot(snapshot)?;
        before_delete();
        remove_planned_snapshot(snapshot, &plan)?;
        Ok(plan.files)
    }

    fn open_directory(path: &Path) -> io::Result<OwnedFd> {
        open(path, DIRECTORY_FLAGS, Mode::empty()).map_err(io::Error::from)
    }

    fn open_directory_at(parent: &OwnedFd, name: &str) -> io::Result<OwnedFd> {
        openat(parent, name, DIRECTORY_FLAGS, Mode::empty()).map_err(io::Error::from)
    }

    fn open_or_create_root(root: &Path) -> io::Result<OwnedFd> {
        let (parent_path, root_name) = split_root(root)?;
        fs::create_dir_all(&parent_path)?;
        let parent = open(&parent_path, DIRECTORY_FLAGS, Mode::empty()).map_err(io::Error::from)?;
        ensure_directory_at(&parent, &root_name)
    }

    fn open_directories(root: &OwnedFd, directories: &[&str], create: bool) -> io::Result<OwnedFd> {
        let mut current = rustix::io::dup(root).map_err(io::Error::from)?;
        for name in directories {
            current = if create {
                ensure_directory_at(&current, name)?
            } else {
                open_directory_at(&current, name)?
            };
        }
        Ok(current)
    }

    fn ensure_directory_at(parent: &OwnedFd, name: &str) -> io::Result<OwnedFd> {
        match mkdirat(parent, name, Mode::RWXU) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => open_directory_at(parent, name),
            Err(error) => Err(error.into()),
        }
    }

    fn read_regular_at<P: rustix::path::Arg>(
        directory: &OwnedFd,
        name: P,
    ) -> io::Result<(Vec<u8>, u64)> {
        let file_fd =
            openat(directory, name, READ_FLAGS, Mode::empty()).map_err(io::Error::from)?;
        let metadata = fstat(&file_fd).map_err(io::Error::from)?;
        if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile {
            return Err(io::Error::other("cache entry is not a regular file"));
        }
        let size = u64::try_from(metadata.st_size).unwrap_or_default();
        let mut bytes = Vec::new();
        fs::File::from(file_fd).read_to_end(&mut bytes)?;
        Ok((bytes, size))
    }

    fn visit_best_effort(
        directory: &OwnedFd,
        visit: &mut impl FnMut(&OwnedFd, &CStr, FileType, u64),
    ) {
        let Ok(entries) = Dir::read_from(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if is_dot(name) {
                continue;
            }
            let Ok(metadata) = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) else {
                continue;
            };
            let file_type = FileType::from_raw_mode(metadata.st_mode);
            if file_type == FileType::Directory {
                if let Ok(child) = openat(directory, name, DIRECTORY_FLAGS, Mode::empty()) {
                    visit_best_effort(&child, visit);
                }
            } else {
                visit(
                    directory,
                    name,
                    file_type,
                    u64::try_from(metadata.st_size).unwrap_or_default(),
                );
            }
        }
    }

    fn prune_directory(
        directory: &OwnedFd,
        now: SystemTime,
        layer: CacheLayer,
        before_leaf: &mut impl FnMut(&CStr),
    ) -> usize {
        let Ok(entries) = Dir::read_from(directory) else {
            return 0;
        };
        let mut removed = 0_usize;
        for entry in entries.flatten() {
            let name = entry.file_name();
            if is_dot(name) {
                continue;
            }
            let Ok(metadata) = statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) else {
                continue;
            };
            let file_type = FileType::from_raw_mode(metadata.st_mode);
            if file_type == FileType::Directory {
                if let Ok(child) = openat(directory, name, DIRECTORY_FLAGS, Mode::empty()) {
                    removed =
                        removed.saturating_add(prune_directory(&child, now, layer, before_leaf));
                }
                continue;
            }
            before_leaf(name);
            let is_fresh = file_type == FileType::RegularFile
                && read_regular_at(directory, name)
                    .ok()
                    .and_then(|(bytes, _)| classify_entry(&bytes, now, layer))
                    == Some(Freshness::Fresh);
            if !is_fresh && unlinkat(directory, name, AtFlags::empty()).is_ok() {
                removed = removed.saturating_add(1);
            }
        }
        removed
    }

    fn pin_snapshot(snapshot: &Snapshot) -> io::Result<RemovalPlan> {
        verify_identity(&snapshot.parent, snapshot.name.as_str(), snapshot.identity)?;
        let Some(directory) = snapshot.directory.as_ref() else {
            return Ok(RemovalPlan {
                children: Vec::new(),
                files: 1,
            });
        };
        verify_handle_identity(directory, snapshot.identity)?;
        let pinned = pin_directory(directory)?;
        Ok(RemovalPlan {
            children: pinned.children,
            files: pinned.files,
        })
    }

    fn pin_directory(directory: &OwnedFd) -> io::Result<PinnedDirectory> {
        let entries = Dir::read_from(directory).map_err(io::Error::from)?;
        let mut children = Vec::new();
        let mut files = 0_usize;
        for entry in entries {
            let entry = entry.map_err(io::Error::from)?;
            let name = entry.file_name();
            if is_dot(name) {
                continue;
            }
            let metadata =
                statat(directory, name, AtFlags::SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
            let identity = identity_from_stat(&metadata);
            let child_directory = if identity.file_type == FileType::Directory {
                let handle = openat(directory, name, DIRECTORY_FLAGS, Mode::empty())
                    .map_err(io::Error::from)?;
                verify_handle_identity(&handle, identity)?;
                let child = pin_directory(&handle)?;
                files = files.saturating_add(child.files);
                Some(PinnedDirectory {
                    handle,
                    children: child.children,
                    files: child.files,
                })
            } else {
                files = files.saturating_add(1);
                None
            };
            children.push(PinnedEntry {
                name: name.to_owned(),
                identity,
                directory: child_directory,
            });
        }
        Ok(PinnedDirectory {
            handle: rustix::io::dup(directory).map_err(io::Error::from)?,
            children,
            files,
        })
    }

    fn remove_planned_snapshot(snapshot: &Snapshot, plan: &RemovalPlan) -> io::Result<()> {
        verify_identity(&snapshot.parent, snapshot.name.as_str(), snapshot.identity)?;
        if let Some(directory) = snapshot.directory.as_ref() {
            remove_pinned_entries(directory, &plan.children)?;
            verify_identity(&snapshot.parent, snapshot.name.as_str(), snapshot.identity)?;
            unlinkat(&snapshot.parent, snapshot.name.as_str(), AtFlags::REMOVEDIR)
                .map_err(io::Error::from)
        } else {
            unlinkat(&snapshot.parent, snapshot.name.as_str(), AtFlags::empty())
                .map_err(io::Error::from)
        }
    }

    fn remove_pinned_entries(directory: &OwnedFd, entries: &[PinnedEntry]) -> io::Result<()> {
        for entry in entries {
            verify_identity(directory, entry.name.as_c_str(), entry.identity)?;
            if let Some(child) = entry.directory.as_ref() {
                verify_handle_identity(&child.handle, entry.identity)?;
                remove_pinned_entries(&child.handle, &child.children)?;
                verify_identity(directory, entry.name.as_c_str(), entry.identity)?;
                unlinkat(directory, entry.name.as_c_str(), AtFlags::REMOVEDIR)
                    .map_err(io::Error::from)?;
            } else {
                unlinkat(directory, entry.name.as_c_str(), AtFlags::empty())
                    .map_err(io::Error::from)?;
            }
        }
        Ok(())
    }

    fn verify_identity<P: rustix::path::Arg>(
        parent: &OwnedFd,
        name: P,
        expected: Identity,
    ) -> io::Result<()> {
        let metadata = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) => metadata,
            Err(rustix::io::Errno::NOENT) => return Err(snapshot_changed()),
            Err(error) => return Err(error.into()),
        };
        if identity_from_stat(&metadata) == expected {
            Ok(())
        } else {
            Err(snapshot_changed())
        }
    }

    fn verify_handle_identity(handle: &OwnedFd, expected: Identity) -> io::Result<()> {
        let metadata = fstat(handle).map_err(io::Error::from)?;
        if identity_from_stat(&metadata) == expected {
            Ok(())
        } else {
            Err(snapshot_changed())
        }
    }

    fn identity_from_stat(metadata: &rustix::fs::Stat) -> Identity {
        Identity {
            device: metadata.st_dev as u128,
            inode: metadata.st_ino as u128,
            file_type: FileType::from_raw_mode(metadata.st_mode),
        }
    }

    fn snapshot_changed() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "cache snapshot changed during removal",
        )
    }

    #[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
    fn rename_noreplace(parent: &OwnedFd, old: &str, new: &str) -> io::Result<()> {
        renameat_with(parent, old, parent, new, RenameFlags::NOREPLACE).map_err(io::Error::from)
    }

    #[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
    fn rename_noreplace(_parent: &OwnedFd, _old: &str, _new: &str) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe cache clear is not supported on this platform",
        ))
    }

    fn split_root(root: &Path) -> io::Result<(PathBuf, String)> {
        let parent = root
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let Some(name) = root.file_name().and_then(|name| name.to_str()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cache root has no valid file name",
            ));
        };
        Ok((parent, name.to_owned()))
    }

    fn is_dot(name: &CStr) -> bool {
        name.to_bytes() == b"." || name.to_bytes() == b".."
    }

    #[cfg(test)]
    pub(crate) fn read_with_after_root_open(
        root: &Path,
        directories: &[&str],
        name: &str,
        after_root_open: impl FnOnce(),
    ) -> io::Result<Vec<u8>> {
        let root_fd = open_directory(root)?;
        after_root_open();
        let directory = open_directories(&root_fd, directories, false)?;
        read_regular_at(&directory, name).map(|(bytes, _)| bytes)
    }

    #[cfg(test)]
    pub(crate) fn read_with_before_file_open(
        root: &Path,
        directories: &[&str],
        name: &str,
        before_file_open: impl FnOnce(),
    ) -> io::Result<Vec<u8>> {
        let root_fd = open_directory(root)?;
        let directory = open_directories(&root_fd, directories, false)?;
        before_file_open();
        read_regular_at(&directory, name).map(|(bytes, _)| bytes)
    }

    #[cfg(test)]
    pub(crate) fn remove_snapshot_with_delete_hook(
        snapshot: &Snapshot,
        before_delete: impl FnOnce(),
    ) -> io::Result<usize> {
        remove_snapshot_with_before_delete(snapshot, before_delete)
    }

    #[cfg(test)]
    pub(crate) fn detach_root_with_rename_hook(
        root: &Path,
        before_rename: impl FnOnce(),
    ) -> io::Result<Option<Snapshot>> {
        detach_root_with_before_rename(root, before_rename)
    }

    #[cfg(test)]
    pub(crate) fn prune_with_before_leaf(
        root: &Path,
        layer_name: &str,
        now: SystemTime,
        layer: CacheLayer,
        mut before_leaf: impl FnMut(&CStr),
    ) -> usize {
        let Ok(root_fd) = open_directory(root) else {
            return 0;
        };
        let Ok(layer_fd) = open_directory_at(&root_fd, layer_name) else {
            return 0;
        };
        prune_directory(&layer_fd, now, layer, &mut before_leaf)
    }
}

#[cfg(not(unix))]
mod imp {
    use std::io;
    use std::path::Path;
    use std::time::SystemTime;

    use super::{CacheLayer, CacheLayerStats};

    pub(crate) struct Snapshot;

    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "safe cache file operations are not supported on this platform",
        )
    }

    pub(crate) fn read(_root: &Path, _directories: &[&str], _name: &str) -> io::Result<Vec<u8>> {
        Err(unsupported())
    }

    pub(crate) fn write(
        _root: &Path,
        _directories: &[&str],
        _name: &str,
        _bytes: &[u8],
    ) -> io::Result<()> {
        Err(unsupported())
    }

    pub(crate) fn remove(_root: &Path, _directories: &[&str], _name: &str) -> io::Result<()> {
        Err(unsupported())
    }

    pub(crate) fn collect_stats(
        _root: &Path,
        _layer_name: &str,
        _now: SystemTime,
        _layer: CacheLayer,
    ) -> CacheLayerStats {
        CacheLayerStats::default()
    }

    pub(crate) fn prune(
        _root: &Path,
        _layer_name: &str,
        _now: SystemTime,
        _layer: CacheLayer,
    ) -> usize {
        0
    }

    pub(crate) fn detach_root(_root: &Path) -> io::Result<Option<Snapshot>> {
        Err(unsupported())
    }

    pub(crate) fn remove_snapshot(_snapshot: &Snapshot) -> io::Result<usize> {
        Err(unsupported())
    }
}

pub(super) use imp::{collect_stats, detach_root, prune, read, remove, remove_snapshot, write};

#[cfg(all(test, unix))]
pub(super) use imp::{
    detach_root_with_rename_hook, prune_with_before_leaf, read_with_after_root_open,
    read_with_before_file_open, remove_snapshot_with_delete_hook,
};
