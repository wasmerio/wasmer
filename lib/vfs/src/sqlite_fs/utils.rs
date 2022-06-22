use crate::sqlite_fs::*;
use crate::{FsError, KeyType};
use libc::{c_int, ino_t, mode_t, off_t, size_t, uid_t};
use rusqlite::OptionalExtension;
use std::convert::{TryFrom, TryInto};

impl SqliteFs {
    /// Set a new size for key given by `path`.
    pub fn truncate(&mut self, path: Key<'_>, size: off_t, uid: uid_t, guid: uid_t) -> Result<()> {
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        tx.check_parent_access(path, uid, guid)?;
        tx.check_write(path, uid, guid)?;
        tx.truncate(path, size)?;
        tx.0.commit()?;
        Ok(())
    }

    /// Read from key `path` into `buf`.
    pub fn read(
        &mut self,
        path: Key<'_>,
        buf: &mut [u8],
        offset: off_t,
        open_flags: c_int,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<size_t> {
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        let ret = tx.read(path, buf, offset, open_flags, uid, guid)?;
        tx.0.commit()?;
        Ok(ret)
    }

    /// Write into key `path` from `buf`.
    pub fn write(
        &mut self,
        path: Key<'_>,
        buf: &[u8],
        offset: off_t,
        open_flags: c_int,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<size_t> {
        let default_mode = self.default_mode;
        let mut lck = self.inner.lock().unwrap();
        let tx = lck.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut tx = Transaction(tx);
        let ret = tx.write(path, buf, offset, open_flags, default_mode, uid, guid)?;
        tx.0.commit()?;
        Ok(ret)
    }
}

impl<'tx> Transaction<'tx> {
    /// Inner method to fetch the maximum inode from the database.
    pub fn get_max_inode(&mut self) -> Result<Option<ino_t>> {
        let mut stmt = self
            .0
            .prepare_cached("select COALESCE(max(inode), 0) from meta_data;")?;
        let max: ino_t = stmt.query_row([], |row| row.get(0))?;
        if max == 0 {
            return Ok(None);
        }
        Ok(Some(max))
    }

    /// Inner method to return an unused inode value for new files.
    pub fn get_new_inode(&mut self) -> Result<ino_t> {
        let max = self.get_max_inode()?.unwrap_or(0);
        Ok(max + 1)
    }

    /// Inner method to return whether a given key exists. The return value is the key's size.
    pub fn key_exists(&mut self, key: Key<'_>) -> Result<Option<size_t>> {
        let mut stmt = self
            .0
            .prepare_cached("select size from meta_data where key = :key;")?;
        let res: Option<size_t> = stmt
            .query_row(named_params! { ":key": key }, |row| row.get(0))
            .optional()?;
        Ok(res)
    }

    /// Inner method to return whether a given key exists and is a directory.
    pub fn key_is_dir(&mut self, key: Key<'_>) -> Result<Option<bool>> {
        let mut stmt = self
            .0
            .prepare_cached("select type == :typedir from meta_data where key = :key;")?;
        let res: Option<bool> = stmt
            .query_row(named_params! { ":key": key, ":typedir": TYPE_DIR }, |row| {
                row.get(0)
            })
            .optional()?;
        Ok(res)
    }

    /// Inner method to get the metadata of a given key.
    pub fn getmetadata(&mut self, path: Key<'_>) -> Result<Metadata> {
        let mut stmt = self.0.prepare_cached("select mode, uid, gid, atime, mtime, ctime, size, inode, type from meta_data where key = :key;")?;

        let metadata: Metadata = stmt.query_row(named_params! { ":key": path }, |row| {
            Ok(Metadata {
                mode: row.get(0)?,
                uid: row.get(1)?,
                gid: row.get(2)?,
                atime: row.get(3)?,
                mtime: row.get(4)?,
                ctime: row.get(5)?,
                size: row.get(6)?,
                inode: row.get(7)?,
                type_: row.get(8)?,
            })
        })?;

        Ok(metadata)
    }

    /// Inner method to get the metadata of a given key.
    pub fn get_size_and_type(&mut self, path: Key<'_>) -> Result<Option<(size_t, KeyType)>> {
        let mut stmt = self
            .0
            .prepare_cached("select size, type from meta_data where key = :key;")?;

        let size_and_type = stmt
            .query_row(named_params! { ":key": path }, |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;

        Ok(size_and_type)
    }

    /// Inner method to set the metadata of a given key.
    pub fn setmetadata(&mut self, path: Key<'_>, new_val: Metadata) -> Result<()> {
        {
            let mut stmt = self.0.prepare_cached("update or abort meta_data set mode = :mode, uid = :uid, gid = :gid, atime = :atime, mtime = :mtime, ctime = :ctime, size = :size, inode = :inode where key = :key;")?;

            stmt.execute(named_params! {
                ":key": path,
                ":mode": new_val.mode,
                ":uid": new_val.uid,
                ":gid": new_val.gid,
                ":atime": new_val.atime,
                ":mtime": new_val.mtime,
                ":ctime": new_val.ctime,
                ":size": new_val.size,
                ":inode": new_val.inode,
            })?;
        }

        Ok(())
    }

    /// Inner method to rename a key.
    pub fn rename_key(&mut self, old: Key<'_>, new: Key<'_>) -> Result<()> {
        {
            let mut stmt1 = self
                .0
                .prepare_cached("update meta_data set key = :new where key = :old;")?;
            stmt1.execute(named_params! {
                ":new": new,
                ":old": old,
            })?;
        }
        {
            let mut stmt2 = self
                .0
                .prepare_cached("update value_data set key = :new where key = :old;")?;
            stmt2.execute(named_params! {
                ":new": new,
                ":old": old,
            })?;
        }
        Ok(())
    }

    /// Inner method to remove a key.
    pub fn remove_key(&mut self, path: Key<'_>) -> Result<()> {
        {
            let mut stmt1 = self
                .0
                .prepare_cached("delete from meta_data where key = :key;")?;
            stmt1.execute(named_params! {
                ":key": path,
            })?;
        }
        {
            let mut stmt2 = self
                .0
                .prepare_cached("delete from value_data where key = :key;")?;
            stmt2.execute(named_params! {
                ":key": path,
            })?;
        }
        Ok(())
    }

    /// Inner method to create a key's metadata with given metadata. This will fail if the key
    /// already exists.
    pub fn createmetadata(&mut self, path: Key<'_>, new_val: Metadata) -> Result<()> {
        {
            let mut stmt = self.0.prepare_cached("insert or abort into meta_data (key, type, mode, uid, gid, atime, mtime, ctime, size, inode) values (:key, :type, :mode, :uid, :gid, :atime, :mtime, :ctime, :size, :inode);")?;

            stmt.execute(named_params! {
                ":key": path,
                ":type": new_val.type_,
                ":mode": new_val.mode,
                ":uid": new_val.uid,
                ":gid": new_val.gid,
                ":atime": new_val.atime,
                ":mtime": new_val.mtime,
                ":ctime": new_val.ctime,
                ":size": new_val.size,
                ":inode": new_val.inode,
            })?;
        }

        Ok(())
    }

    /// Inner method to ensure a user and group id can access the parent directory of a path.
    #[inline(always)]
    pub fn check_parent_access(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        let mut path = path.as_ref();
        while let Some(parent) = path.parent() {
            self.access(parent.into(), libc::X_OK, uid, guid, None)?;
            path = parent;
        }
        Ok(())
    }

    /// Inner method to ensure a user and group id can write to the parent directory of a path.
    #[inline(always)]
    pub fn check_parent_write(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        let mut path = path.as_ref();
        while let Some(parent) = path.parent() {
            self.check_parent_access(parent.into(), uid, guid)?;
            self.access(parent.into(), libc::W_OK | libc::X_OK, uid, guid, None)?;
            path = parent;
        }
        Ok(())
    }

    /// Inner method to ensure a user and group id can read a path.
    #[inline(always)]
    pub fn check_read(
        &mut self,
        path: Key<'_>,
        uid: uid_t,
        guid: uid_t,
        key_exists: Option<Option<size_t>>,
    ) -> Result<()> {
        self.access(path, libc::R_OK | libc::F_OK, uid, guid, key_exists)
    }

    /// Inner method to ensure a user and group id can write to a path.
    #[inline(always)]
    pub fn check_write(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        self.access(path, libc::W_OK | libc::F_OK, uid, guid, None)
    }

    /// Inner method to ensure a user and group id can access and write to a directory path.
    #[inline(always)]
    pub fn check_dir_write(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        self.access(path, libc::W_OK | libc::F_OK | libc::X_OK, uid, guid, None)
    }

    /// Inner method to ensure a user and group id can access and read a directory path.
    #[inline(always)]
    pub fn check_dir_read(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        self.access(path, libc::R_OK | libc::F_OK | libc::X_OK, uid, guid, None)
    }

    /// Inner method to ensure a user and group id can read the parent directory of a path.
    #[inline(always)]
    pub fn check_parent_read(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            self.access(parent.into(), libc::R_OK | libc::X_OK, uid, guid, None)
        } else {
            Ok(())
        }
    }

    /// Inner method to update a key's access, modified and change time stamps to current time.
    pub fn key_modified(&mut self, path: Key<'_>) -> Result<()> {
        let mut now: libc::time_t = 0;
        unsafe { libc::time(&mut now as *mut libc::time_t) };
        {
            /* key is modified (mtime) so atime and ctime must be modified as well. */
            let mut stmt = self.0.prepare_cached("update meta_data set atime = :atime, mtime = :mtime, ctime = :ctime where key = :key;")?;
            stmt.execute(named_params! {
                ":key": path,
                ":atime": now,
                ":mtime": now,
                ":ctime": now,
            })?;
        }
        Ok(())
    }

    #[inline(always)]
    /// Deletes an entry given by key `path`.
    pub fn unlink(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        self.check_parent_write(path, uid, guid)?;
        match self.key_exists(path)? {
            Some(_) => {}
            None => return Err(Error::DoesNotExist(path.into()).into()),
        };
        if self.key_is_dir(path)?.unwrap_or(false) {
            return Err(Error::IsDirectory(path.into()).into());
        }
        self.remove_key(path)?;
        Ok(())
    }

    #[inline(always)]
    /// Removes the directory at key given by `path`.
    pub fn rmdir(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        self.check_parent_write(path, uid, guid)?;
        if self.get_dir_children_num(path)? > 0 {
            return Err(Error::DirectoryNotEmpty(path.into()).into());
        }

        self.remove_key(path)?;
        Ok(())
    }

    #[inline(always)]
    /// Creates a new symbolink link `to` that points to the key `path`.
    pub fn symlink(
        &mut self,
        path: Key<'_>,
        to: Key<'_>,
        mut mode: mode_t,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<()> {
        self.check_parent_write(to, uid, guid)?;
        if self.getmetadata(to).is_ok() {
            return Err(Error::AlreadyExists(path.into()).into());
        }
        mode &= !(libc::S_IFMT); // Ignore non-permission bits
        mode |= libc::S_IFLNK; // Set symbolic link type bit
        let metadatas = Metadata {
            mode,
            uid,
            gid: guid,
            atime: 0,
            mtime: 0,
            ctime: 0,
            size: 0,
            inode: self.get_new_inode()?,
            type_: KeyType::SymLink,
        };
        self.createmetadata(to, metadatas)?;

        let data = path.as_ref().to_str().expect("Non-utf8 path").as_bytes();

        self.set_value(to, data, 0, 0)?;
        Ok(())
    }

    #[inline(always)]
    /// Renames the file or directory given by key `from` to `to`. If `from` is a directory, its
    /// sub-hierarchy will be renamed as well.
    pub fn rename(&mut self, from: Key<'_>, to: Key<'_>, uid: uid_t, guid: uid_t) -> Result<()> {
        self.check_parent_write(from, uid, guid)?;
        self.check_parent_write(to, uid, guid)?;
        match self.key_exists(from)? {
            Some(_) => {}
            None => return Err(Error::DoesNotExist(from.into()).into()),
        };

        let to_is_dir: bool = self.key_is_dir(to)?.unwrap_or(false);
        let from_is_dir: bool = self.key_is_dir(from)?.unwrap_or(false);

        if to_is_dir && !from_is_dir {
            return Err(Error::IsDirectory(to.into()).into());
        }
        /* "'from' can specify a directory.  In this case, 'to' must either not exist,
         * or it must specify an empty directory" - (man 2 rename.)
         */
        if from_is_dir {
            if self.key_exists(to)?.is_some() {
                if !to_is_dir {
                    return Err(Error::NotADirectory(to.into()).into());
                } else if self.get_dir_children_num(to)? > 0 {
                    return Err(Error::DirectoryNotEmpty(to.into()).into());
                }
            }
            self.rename_dir_children(from, to, uid, guid)?;
        }

        if self.key_exists(to)?.is_some() {
            self.remove_key(to)?;
        }
        self.rename_key(from, to)?;
        Ok(())
    }

    #[inline(always)]
    /// Get the path pointed to by symbolic link `path`.
    pub fn readlink(&mut self, path: Key<'_>, uid: uid_t, guid: uid_t) -> Result<PathBuf> {
        self.check_parent_access(path, uid, guid)?;
        self.check_read(path, uid, guid, None)?;
        let metadata = self.getmetadata(path)?;
        if metadata.type_ != KeyType::SymLink {
            return Err(Error::InvalidArgument.into());
        }
        let key_exists = self.key_exists(path)?;
        let size = match key_exists {
            None => return Err(Error::DoesNotExist(path.into()).into()),
            Some(v) => v,
        };
        let mut buf = vec![0; size];
        self.get_value(path, &mut buf, 0, size.try_into()?, key_exists)?;
        let path_string: String = match String::from_utf8(buf) {
            Ok(s) => s,
            Err(_) => return Err(Error::InvalidArgument.into()),
        };
        let ret_val: PathBuf = PathBuf::from(path_string);
        Ok(ret_val)
    }

    #[inline(always)]
    /// Creates a new directory at key given by `path`. On success returns the inode number of the
    /// directory.
    pub fn mkdir(
        &mut self,
        path: Key<'_>,
        mut mode: mode_t,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<ino_t> {
        self.check_parent_write(path, uid, guid)?;
        if self.key_exists(path)?.is_some() {
            return Err(Error::AlreadyExists(path.into()).into());
        }
        mode &= !(libc::S_IFMT); // Ignore non-permission bits
        mode |= libc::S_IFDIR; // Set directory type bit

        let metadatas = Metadata {
            mode,
            uid,
            gid: guid,
            atime: 0,
            mtime: 0,
            ctime: 0,
            size: 0,
            inode: self.get_new_inode()?,
            type_: KeyType::Dir,
        };
        self.createmetadata(path, metadatas)?;
        Ok(metadatas.inode)
    }

    fn rename_dir_children(
        &mut self,
        old: Key<'_>,
        new: Key<'_>,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<()> {
        self.check_parent_access(old, uid, guid)?;
        self.check_dir_read(old, uid, guid)?;

        let old_is_dir: bool = self.key_is_dir(old)?.unwrap_or(false);
        if !old_is_dir {
            return Err(Error::NotADirectory(old.into()).into());
        }

        let mut children = vec![];
        let mut lpath = old.as_ref().to_str().expect("Non-utf8 path").to_string();
        if lpath.ends_with('/') {
            lpath.pop();
        }
        let mut rpath = new.as_ref().to_str().expect("Non-utf8 path").to_string();
        if rpath.ends_with('/') {
            rpath.pop();
        }

        {
            let mut stmt = self
                .0
                .prepare_cached("select key, mode from meta_data where key glob :pattern;")?;
            let iter = stmt.query_map(
                named_params! { ":pattern": &format!("{}/*", lpath) },
                |row| {
                    let key: String = row.get(0)?;
                    let mode: mode_t = row.get(1)?;
                    Ok((key, mode))
                },
            )?;
            for result in iter {
                let (key, mode) = result?;
                children.push((key, mode));
            }
        }

        for (child_path, _child_mode) in children {
            if child_path == lpath {
                continue;
            }

            let child_filename = &child_path[lpath.len() + 1..];
            if child_filename.is_empty() {
                /* special case when dir the root directory */
                continue;
            }

            let new_path = format!("{}/{}", rpath, child_filename);
            let new_key = Path::new(&new_path);
            if self.key_exists(new_key.into())?.is_some() {
                self.remove_key(new_key.into())?;
            }

            self.rename_key(Path::new(&child_path).into(), new_key.into())?;
        }

        Ok(())
    }

    #[inline(always)]
    /// Inner method to get the number of child entries in a directory.
    pub fn get_dir_children_num(&mut self, path: Key<'_>) -> Result<usize> {
        let mut count = 0;
        let path_is_dir: bool = self.key_is_dir(path)?.unwrap_or(false);
        if !path_is_dir {
            return Ok(count);
        }
        let mut lpath = path.as_ref().to_str().expect("Non-utf8 path").to_string();
        if lpath.ends_with('/') {
            lpath.pop();
        }
        {
            let mut stmt = self
                .0
                .prepare_cached("select key from meta_data where key glob :pattern;")?;
            let iter = stmt.query_map(
                named_params! { ":pattern": &format!("{}/*", lpath) },
                |row| {
                    // FIXME: avoid allocation here by using get_ref
                    let key: String = row.get(0)?;
                    Ok(key[lpath.len() + 2..].contains('/'))
                },
            )?;
            for result in iter {
                let contains: bool = result?;
                if contains {
                    count += 1;
                }
            }
        }
        Ok(count)
    }

    #[inline(always)]
    /// Inner version of [`access`][`crate::Connection::access`] that uses an `sqlite3`
    /// transaction.
    // TODO: go through logic again and verify correct behavior
    pub fn access(
        &mut self,
        path: Key<'_>,
        mask: c_int,
        uid: uid_t,
        guid: uid_t,
        key_exists: Option<Option<size_t>>,
    ) -> Result<()> {
        let key_exists = match key_exists {
            Some(n) => n,
            None => self.key_exists(path)?,
        };
        if uid == 0 {
            /* root user so everything is granted */
            return match key_exists {
                None => Err(Error::DoesNotExist(path.into()).into()),
                Some(_) => Ok(()),
            };
        }

        /* F_OK tests for the existence of the file. */
        if mask == libc::F_OK {
            /* access(2)
             *
             * A file is accessible only if the permissions on each of the
             * directories in the path prefix of pathname grant search (i.e.,
             * execute) access.  If any directory is inaccessible, then the
             * access() call fails, regardless of the permissions on the file
             * itself.
             */
            let mut path_iter = path.as_ref();
            while let Some(parent) = path_iter.parent() {
                path_iter = parent;
                let metadata = self.getmetadata(parent.into())?;
                let mut allowed = false;
                if uid == metadata.uid && (metadata.mode & libc::S_IXUSR > 0) {
                    allowed |= true;
                }
                if guid == metadata.gid && (metadata.mode & libc::S_IXGRP > 0) {
                    allowed |= true;
                }
                if metadata.mode & libc::S_IXOTH > 0 {
                    allowed |= true;
                }
                if !allowed {
                    return Err(Error::PermissionDenied(path.into()).into());
                }
            }

            match key_exists {
                None => Err(Error::DoesNotExist(path.into()).into()),
                Some(_) => Ok(()),
            }
        } else {
            let metadata = self.getmetadata(path)?;
            if uid == metadata.uid {
                if ((mask & libc::R_OK > 0) && libc::S_IRUSR & metadata.mode <= 0)
                    || ((mask & libc::W_OK > 0) && libc::S_IWUSR & metadata.mode <= 0)
                    || ((mask & libc::X_OK > 0) && libc::S_IXUSR & metadata.mode <= 0)
                {
                    return Err(Error::PermissionDenied(path.into()).into()); // -EACCES;
                }
            } else if guid == metadata.gid {
                // FIXME || (gid_in_supp_groups(fgid)))
                if ((mask & libc::R_OK > 0) && libc::S_IRGRP & metadata.mode <= 0)
                    || ((mask & libc::W_OK > 0) && libc::S_IWGRP & metadata.mode <= 0)
                    || ((mask & libc::X_OK > 0) && libc::S_IXGRP & metadata.mode <= 0)
                {
                    return Err(Error::PermissionDenied(path.into()).into()); // -EACCES;
                }
            } else if ((mask & libc::R_OK > 0) && libc::S_IROTH & metadata.mode <= 0)
                || ((mask & libc::W_OK > 0) && libc::S_IWOTH & metadata.mode <= 0)
                || ((mask & libc::X_OK > 0) && libc::S_IXOTH & metadata.mode <= 0)
            {
                return Err(Error::PermissionDenied(path.into()).into()); // -EACCES;
            }
            Ok(())
        }
    }

    /// Read from key `path` into `buf`.
    pub fn read(
        &mut self,
        path: Key<'_>,
        buf: &mut [u8],
        offset: off_t,
        open_flags: c_int,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<size_t> {
        let size_and_type = self.get_size_and_type(path)?;
        let key_exists = size_and_type.map(|(size, _)| size as size_t);
        let key_is_dir = size_and_type.map(|(_, type_)| type_ == KeyType::Dir);
        self.check_parent_access(path, uid, guid)?;
        self.check_read(path, uid, guid, Some(key_exists))?;

        match key_is_dir {
            Some(true) => return Err(Error::IsDirectory(path.into()).into()),
            Some(false) => {}
            None => return Err(Error::DoesNotExist(path.into()).into()),
        }

        if open_flags != libc::O_RDONLY && open_flags & libc::O_RDWR == 0 {
            return Err(Error::BadOpenFlags.into());
        }

        let existing_size = match key_exists {
            None => return Err(Error::DoesNotExist(path.into()).into()),
            Some(v) => v,
        };

        if offset >= existing_size.try_into()? {
            /* nothing to read */
            return Ok(0);
        }
        self.get_value(
            path,
            buf,
            offset,
            offset + <off_t as TryFrom<usize>>::try_from(buf.len())?,
            key_exists,
        )
    }

    #[inline(always)]
    /// Write into key `path` from `buf`.
    pub fn write(
        &mut self,
        path: Key<'_>,
        buf: &[u8],
        offset: off_t,
        open_flags: c_int,
        default_mode: mode_t,
        uid: uid_t,
        guid: uid_t,
    ) -> Result<size_t> {
        match self.key_is_dir(path)? {
            Some(false) => {}
            Some(true) => return Err(Error::IsDirectory(path.into()).into()),
            None => {}
        }

        if open_flags & (libc::O_WRONLY | libc::O_RDWR) == 0 {
            return Err(Error::BadOpenFlags.into());
        }

        let existing_size = match self.key_exists(path)? {
            None => {
                // path to write to does not exist
                self.check_parent_write(path, uid, guid)?;
                let metadatas = Metadata {
                    mode: default_mode as _, /* use default mode */
                    uid: uid as _,
                    gid: guid as _,
                    inode: self.get_new_inode()?,
                    atime: 0,
                    mtime: 0,
                    ctime: 0,
                    size: 0,
                    type_: KeyType::Blob,
                };
                self.createmetadata(path, metadatas)?;
                0
            }
            Some(v) => {
                // path to write to already exists
                self.check_parent_access(path, uid, guid)?;
                self.check_write(path, uid, guid)?;
                v
            }
        } as size_t;

        let write_begin: size_t;
        let write_end: size_t;
        if open_flags & libc::O_APPEND != 0 {
            /* handle O_APPEND'ing to an existing file. When O_APPEND is set,
            ignore offset, since that's what POSIX does in a similar situation.
            For more info: https://dev.guardianproject.info/issues/250 */
            write_begin = existing_size;
            write_end = existing_size + buf.len();
        } else if offset > existing_size as _ {
            /* handle writes that start after the end of the existing data.  'buf'
            cannot be used directly with set_value() because the buffer given
            to set_value() needs to include any empty space between the end of
            the existing file and the offset. The return value needs to then be
            set to the number of bytes of _data_ written, not the total number
            of bytes written, which would also include that empty space. */
            let final_size: size_t = offset as usize - existing_size + buf.len();
            let mut tmp = vec![0; final_size];

            // Beware: copy_from_slice() panics if slices don't have the same size
            tmp[offset as usize - existing_size..].copy_from_slice(buf);
            //value.size = offset - existing_size + size;
            //value.data = calloc(value.size, sizeof(char));
            //memset(value.data, 0, offset - existing_size);
            //memcpy(value.data + (offset - existing_size), buf, size);
            write_begin = existing_size;
            write_end = buf.len() + offset as usize;
            /* call set_value before tmp is dropped */
            self.set_value(path, &tmp, write_begin, write_end)?;
            return Ok(buf.len());
        } else {
            write_begin = offset as _;
            write_end = buf.len() + offset as usize;
        }
        let _written = self.set_value(path, buf, write_begin, write_end)?;
        debug_assert_eq!(_written, buf.len());
        Ok(buf.len())
    }

    #[inline(always)]
    /// Inner method to change the size (truncate) of a key given by `path`.
    pub fn key_shorten_value(&mut self, path: Key<'_>, new_length: size_t) -> Result<()> {
        match self.key_is_dir(path)? {
            None => return Err(Error::DoesNotExist(path.into()).into()),
            Some(false) => {}
            Some(true) => return Err(Error::IsDirectory(path.into()).into()),
        }
        let l: size_t = match self.key_exists(path)? {
            Some(val) => val,
            None => return Err(Error::DoesNotExist(path.into()).into()),
        };

        assert!(l > new_length);

        let block_no = new_length / BLOCK_SIZE;
        let mut tmp = vec![0; BLOCK_SIZE];
        let i = self.get_value_block(path, &mut tmp, block_no, 0)?;
        assert!(new_length % BLOCK_SIZE <= i);
        self.set_value_block(path, &tmp[..new_length], block_no)?;

        {
            let mut stmt = self.0.prepare_cached(
                "delete from value_data where key = :key and block_no > :block_no;",
            )?;
            stmt.execute(named_params! { ":key": path, ":block_no": block_no })?;
        }

        {
            let mut stmt = self
                .0
                .prepare_cached("update meta_data set size = :size where key = :key;")?;
            stmt.execute(named_params! { ":key": path, ":size": new_length })?;
        }
        {
            self.key_modified(path)?;
        }
        Ok(())
    }

    #[inline(always)]
    /// Inner method to change the value of an entire block of a key given by `path` to the
    /// contents of slice `buf`.
    pub fn set_value_block(
        &mut self,
        path: Key<'_>,
        buf: &[u8],
        block_no: size_t,
    ) -> Result<size_t> {
        if buf.is_empty() {
            // FIXME: Do we want the first block to be deleted if a file's size is truncated to 0?
            // Otherwise when we try to write to it afterwards, there will be no results from
            // value_data for this key.
            if block_no != 0 {
                let mut stmt = self.0.prepare_cached(
                    "delete from value_data where key = :key and block_no = :block_no;",
                )?;
                stmt.execute(named_params! { ":key": path, ":block_no": block_no })?;
            }
            return Ok(0);
        }

        {
            let mut stmt = self.0.prepare_cached(
                "insert or ignore into value_data (key, block_no) values (:key, :block_no);",
            )?;
            stmt.execute(named_params! { ":key": path, ":block_no": block_no })?;
        }

        {
            let mut stmt = self.0.prepare_cached("update value_data set data_block = :data_block where key = :key and block_no = :block_no;")?;
            stmt.execute(
                named_params! { ":key": path, ":block_no": block_no, ":data_block": buf },
            )?;
        }
        Ok(buf.len())
    }

    #[inline(always)]
    /// Inner method to write the contents of `buf` to key given by `path`. `begin` and `end` are
    /// the positions in bytes relative to the file to start and finish writing to.
    pub fn set_value(
        &mut self,
        path: Key<'_>,
        buf: &[u8],
        begin: size_t,
        mut end: size_t,
    ) -> Result<size_t> {
        /* get the size of the file if it already exists */
        let filesize: size_t = match self.key_exists(path)? {
            Some(val) => val,
            None => {
                let mut createfile_stmt = self.0.prepare_cached(
                    "insert or ignore into meta_data (key, size) values (:key, 0);",
                )?;
                createfile_stmt.execute(named_params! { ":key": path })?;
                0
            }
        };

        let mut length: size_t;
        let mut position_in_value: size_t;

        if end == 0 {
            end = begin + buf.len();
        }
        let mut block_no: size_t = begin / BLOCK_SIZE;
        let mut blockbegin: size_t = block_no * BLOCK_SIZE; // 'begin' chopped to BLOCK_SIZE increments
                                                            // beginning of last block, i.e. 'end' rounded to 'BLOCK_SIZE'
        let blockend: size_t = end as usize / BLOCK_SIZE * BLOCK_SIZE;

        let mut tmp = vec![0_u8; BLOCK_SIZE];
        /* partial write in the first block */
        {
            let end_of_this_block: size_t;

            let old_size: size_t = match self.get_value_block(path, &mut tmp, block_no, 0) {
                Ok(v) => v,
                Err(FsError::EntityNotFound) => 0, // Create first block if it's missing.
                Err(err) => return Err(err),
            };
            if end as usize > (blockbegin + BLOCK_SIZE) {
                // the write spans multiple blocks, only write first one
                end_of_this_block = blockbegin + BLOCK_SIZE;
            } else {
                end_of_this_block = end as usize; // the write fits in a single block
            }
            position_in_value = end_of_this_block - begin;

            // Beware: copy_from_slice() panics if slices don't have the same size
            tmp[begin - blockbegin..][..position_in_value]
                .copy_from_slice(&buf[..position_in_value]);
            //memcpy(tmp + (begin - blockbegin), value->data, position_in_value);
            length = end_of_this_block - blockbegin;
            if length < old_size {
                length = old_size;
            }
            self.set_value_block(path, &tmp[..length], block_no)?;
            block_no += 1;
            blockbegin += BLOCK_SIZE;
        }

        while blockbegin < blockend {
            self.set_value_block(path, &buf[position_in_value..][..BLOCK_SIZE], block_no)?;
            block_no += 1;
            blockbegin += BLOCK_SIZE;
            position_in_value += BLOCK_SIZE;
        }

        /* partial block at the end of the write */
        if blockbegin < end as usize {
            assert_eq!(blockbegin % BLOCK_SIZE, 0);
            assert!((end as usize - blockbegin) < BLOCK_SIZE);

            for b in tmp.iter_mut() {
                *b = 0;
            }
            let mut get_value_size: size_t = self
                .get_value_block(path, &mut tmp, block_no, 0)
                .unwrap_or(0);
            let copy_len = std::cmp::min(buf[position_in_value..].len(), end as usize - blockbegin);
            // Beware: copy_from_slice() panics if slices don't have the same size
            tmp[..copy_len].copy_from_slice(&buf[position_in_value..][..copy_len]);
            //memcpy(tmp, value->data + position_in_value, end - blockbegin);
            if get_value_size < (end as usize - blockbegin) {
                get_value_size = end as usize - blockbegin;
            }
            self.set_value_block(path, &tmp[..get_value_size], block_no)?;
        }

        {
            let mut update_size_stmt = self
                .0
                .prepare_cached("update meta_data set size = :size where key = :key;")?;
            update_size_stmt.execute(named_params!{ ":key": path, ":size": if end > filesize { end } else { filesize } })?;
        }

        Ok(buf.len())
    }

    #[inline(always)]
    /// Inner method to get the value of a specific block of key given by `path`.
    pub fn get_value_block(
        &mut self,
        path: Key<'_>,
        buf: &mut [u8],
        block_no: size_t,
        offset: off_t,
    ) -> Result<size_t> {
        let ret = {
            let mut stmt = self.0.prepare_cached(
                "select rowid from value_data where key = :key and block_no = :block_no;",
            )?;
            let rowid: i64 = stmt.query_row(
                named_params! { ":key": path, ":block_no": block_no },
                |row| row.get(0),
            )?;
            let blob = self.0.blob_open(
                rusqlite::DatabaseName::Main,
                "value_data",
                "data_block",
                rowid,
                true,
            )?;

            blob.read_at(buf, offset.try_into()?)?
        };
        Ok(ret)
    }

    #[inline(always)]
    /// Inner method to get the value of a specific key given by `path`. `begin` and `end` are
    /// the positions in bytes relative to the file to start and finish reading from.
    pub fn get_value(
        &mut self,
        path: Key<'_>,
        buf: &mut [u8],
        begin: off_t,
        mut end: off_t,
        key_exists: Option<usize>,
    ) -> Result<size_t> {
        let filesize: size_t = match key_exists {
            Some(val) => val,
            None => return Err(Error::DoesNotExist(path.into()).into()),
        };
        if (end == 0) || (end > <off_t as TryFrom<size_t>>::try_from(filesize)?) {
            end = <off_t as TryFrom<size_t>>::try_from(filesize)?;
        }
        let mut ret = 0;
        let mut data_offset = 0;
        if begin < end {
            let mut block_no: size_t = <size_t as TryFrom<off_t>>::try_from(begin)? / BLOCK_SIZE;
            let mut blockbegin: size_t = block_no * BLOCK_SIZE; // rounded down to nearest block
            let blockend: size_t =
                <usize as TryFrom<off_t>>::try_from(end)? / BLOCK_SIZE * BLOCK_SIZE; // beginning of last block
            let offset: off_t = begin - <off_t as TryFrom<size_t>>::try_from(blockbegin)?;
            {
                /* handle first block, whether it is the whole block, or only part of it */
                let actually_read = self.get_value_block(path, buf, block_no, offset)?;
                ret += actually_read;
                data_offset += actually_read;
                block_no += 1;
                blockbegin += BLOCK_SIZE;
            }
            /* read complete blocks in the middle of the write */
            let mut errored = false;
            while blockbegin < blockend {
                if let Ok(actually_read) =
                    self.get_value_block(path, &mut buf[data_offset..], block_no, 0)
                {
                    block_no += 1;
                    blockbegin += BLOCK_SIZE;
                    ret += actually_read;
                    data_offset += actually_read;
                } else {
                    errored = true;
                    break;
                }
            }
            /* partial block at the end of the read */
            if !errored && (blockbegin < end.try_into()?) {
                //assert(blockbegin % BLOCK_SIZE == 0);
                //assert(end - blockbegin < BLOCK_SIZE);
                let actually_read =
                    self.get_value_block(path, &mut buf[data_offset..], block_no, 0)?;
                ret += actually_read;
            }
        }

        Ok(ret)
    }

    #[inline(always)]
    /// Inner method to truncate a file to new size.
    pub fn truncate(&mut self, path: Key<'_>, size: off_t) -> Result<()> {
        let existing_size: size_t = match self.key_exists(path)? {
            Some(val) => val,
            None => return Err(Error::DoesNotExist(path.into()).into()),
        };
        if existing_size > size as usize {
            self.key_shorten_value(path, size as usize)?;
        } else if existing_size < size as usize {
            let new_size = size as usize - existing_size;
            let data = vec![0; new_size];
            self.set_value(path, &data, existing_size, size as usize)?;
        }
        Ok(())
    }
}
