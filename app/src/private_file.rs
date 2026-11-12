//! Private output files; never follow existing links or truncate an unrelated file.
use std::{fs::OpenOptions, io::Write, path::Path};

pub(crate) fn write_new(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    let result = file.write_all(content).and_then(|_| file.sync_all());
    if result.is_err() { let _ = std::fs::remove_file(path); }
    result
}

pub(crate) fn replace(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
    write_new(&tmp, content)?;
    let result = std::fs::rename(&tmp, path);
    if result.is_err() { let _ = std::fs::remove_file(&tmp); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_output_never_overwrites_existing_data_and_replacement_is_private() {
        let dir = std::env::temp_dir().join(format!("bankingcraft-private-{:016x}", rand::random::<u64>())); std::fs::create_dir(&dir).unwrap();
        let path = dir.join("output"); write_new(&path, b"first").unwrap();
        assert!(write_new(&path, b"second").is_err()); assert_eq!(std::fs::read(&path).unwrap(), b"first");
        replace(&path, b"replacement").unwrap(); assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600); }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1); std::fs::remove_dir_all(dir).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn output_does_not_follow_a_symlink_to_another_file() {
        let dir = std::env::temp_dir().join(format!("bankingcraft-link-{:016x}", rand::random::<u64>())); std::fs::create_dir(&dir).unwrap();
        let target = dir.join("unrelated"); std::fs::write(&target, b"keep").unwrap(); let link = dir.join("export");
        std::os::unix::fs::symlink(&target, &link).unwrap(); assert!(write_new(&link, b"bad").is_err());
        replace(&link, b"safe").unwrap(); assert_eq!(std::fs::read(&target).unwrap(), b"keep"); assert_eq!(std::fs::read(&link).unwrap(), b"safe");
        assert!(!std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink()); std::fs::remove_dir_all(dir).unwrap();
    }
}
