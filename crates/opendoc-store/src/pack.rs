//! Pack files and their binary indexes: writing, reading and validating.

use crate::error::StoreError;
use crate::local_store::process_tag;
use opendoc_core::{digest_bytes, HashRef};
use opendoc_format::{decode_record, encode_record, PackIndexEntryRecord, PackIndexRecord};
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub(crate) fn write_pack_files(
    pack_dir: &Path,
    pack_name: &str,
    objects: &[(HashRef, Vec<u8>)],
) -> Result<PackStats, StoreError> {
    let pack_name = clean_pack_name(pack_name)?;
    fs::create_dir_all(pack_dir)?;
    cleanup_pack_temp_files(pack_dir, pack_name)?;
    let pack_path = pack_dir.join(format!("{pack_name}.pack"));
    let index_path = pack_dir.join(format!("{pack_name}.idx"));
    let tmp_pack = pack_path.with_extension(format!("pack.tmp-{}", process_tag()));
    let tmp_index = index_path.with_extension(format!("idx.tmp-{}", process_tag()));

    let mut sorted = objects.to_vec();
    sorted.sort_by_key(|(left, _)| left.to_string());
    sorted.dedup_by(|(left, _), (right, _)| left == right);
    for (hash, bytes) in &sorted {
        let actual =
            digest_bytes(hash.algorithm(), bytes).map_err(|_| StoreError::UnsupportedHash)?;
        if &actual != hash {
            return Err(StoreError::HashMismatch);
        }
    }

    let mut index = Vec::new();
    let mut offset = 0u64;
    {
        let mut pack = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_pack)?;
        pack.write_all(b"ODP0")?;
        offset += 4;
        for (hash, bytes) in &sorted {
            pack.write_all(bytes)?;
            index.push(PackIndexEntry {
                hash: hash.clone(),
                pack: pack_name.to_string(),
                offset,
                length: bytes.len() as u64,
            });
            offset += bytes.len() as u64;
        }
        pack.sync_all()?;
    }

    fs::write(&tmp_index, encode_pack_index(pack_name, &index))?;
    verify_pack_index_paths(&tmp_pack, &index)?;
    fs::rename(&tmp_pack, &pack_path)?;
    fs::rename(&tmp_index, &index_path)?;
    Ok(PackStats {
        pack: pack_name.to_string(),
        objects: index.len(),
        bytes: offset,
    })
}

pub(crate) fn read_objects_from_pack_dir(
    pack_dir: &Path,
    pack_name: &str,
) -> Result<Vec<(HashRef, Vec<u8>)>, StoreError> {
    let pack_name = clean_pack_name(pack_name)?;
    let index_path = pack_dir.join(format!("{pack_name}.idx"));
    if !index_path.exists() {
        return Ok(Vec::new());
    }
    let pack_path = pack_dir.join(format!("{pack_name}.pack"));
    let index_bytes = fs::read(index_path)?;
    let mut objects = Vec::new();
    for entry in decode_pack_index(&index_bytes)? {
        if entry.pack != pack_name {
            return Err(StoreError::Format(format!(
                "pack index entry targets unexpected pack {}",
                entry.pack
            )));
        }
        objects.push((entry.hash.clone(), read_pack_entry(&pack_path, &entry)?));
    }
    Ok(objects)
}

pub(crate) fn get_packed_from_dir(
    pack_dir: &Path,
    hash: &HashRef,
) -> Result<Option<Vec<u8>>, StoreError> {
    for index_path in pack_index_paths(pack_dir)? {
        let expected_pack = pack_name_from_index_path(&index_path)?;
        let index_bytes = fs::read(&index_path)?;
        for entry in decode_pack_index(&index_bytes)? {
            if entry.pack != expected_pack {
                return Err(StoreError::Format(format!(
                    "pack index file targets unexpected pack {}",
                    entry.pack
                )));
            }
            if &entry.hash != hash {
                continue;
            }
            let pack_path = index_path.with_file_name(format!("{}.pack", entry.pack));
            return Ok(Some(read_pack_entry(&pack_path, &entry)?));
        }
    }
    Ok(None)
}

pub(crate) fn pack_index_paths(pack_dir: &Path) -> Result<Vec<PathBuf>, StoreError> {
    if !pack_dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(pack_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) == Some("idx") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackStats {
    pub pack: String,
    pub objects: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PackIndexEntry {
    pub(crate) hash: HashRef,
    pub(crate) pack: String,
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

pub(crate) fn encode_pack_index(pack_name: &str, entries: &[PackIndexEntry]) -> Vec<u8> {
    let record = PackIndexRecord {
        pack: pack_name.to_string(),
        entries: entries
            .iter()
            .map(|entry| PackIndexEntryRecord {
                hash: entry.hash.clone(),
                offset: entry.offset,
                length: entry.length,
            })
            .collect(),
    };
    encode_record(&record)
}

pub(crate) fn decode_pack_index(value: &[u8]) -> Result<Vec<PackIndexEntry>, StoreError> {
    let record: PackIndexRecord =
        decode_record(value).map_err(|err| StoreError::Format(err.to_string()))?;
    record
        .validate()
        .map_err(|err| StoreError::Format(err.to_string()))?;
    let pack = clean_pack_name(&record.pack)?.to_string();
    Ok(record
        .entries
        .into_iter()
        .map(|entry| PackIndexEntry {
            hash: entry.hash,
            pack: pack.clone(),
            offset: entry.offset,
            length: entry.length,
        })
        .collect())
}

pub(crate) fn verify_pack_index_paths(
    pack_path: &Path,
    index: &[PackIndexEntry],
) -> Result<(), StoreError> {
    let mut file = fs::File::open(pack_path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    if &magic != b"ODP0" {
        return Err(StoreError::Format("unsupported pack file".to_string()));
    }
    for entry in index {
        validate_pack_entry_range(&file, entry)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        let length = usize::try_from(entry.length).map_err(|_| {
            StoreError::Format("pack index entry length exceeds platform limit".to_string())
        })?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        let actual = digest_bytes(entry.hash.algorithm(), &bytes)
            .map_err(|_| StoreError::UnsupportedHash)?;
        if actual != entry.hash {
            return Err(StoreError::HashMismatch);
        }
    }
    Ok(())
}

pub(crate) fn read_pack_entry(
    pack_path: &Path,
    entry: &PackIndexEntry,
) -> Result<Vec<u8>, StoreError> {
    let mut file = fs::File::open(pack_path)?;
    validate_pack_entry_range(&file, entry)?;
    file.seek(SeekFrom::Start(entry.offset))?;
    let length = usize::try_from(entry.length).map_err(|_| {
        StoreError::Format("pack index entry length exceeds platform limit".to_string())
    })?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)?;
    let actual =
        digest_bytes(entry.hash.algorithm(), &bytes).map_err(|_| StoreError::UnsupportedHash)?;
    if actual != entry.hash {
        return Err(StoreError::HashMismatch);
    }
    Ok(bytes)
}

pub(crate) fn validate_pack_entry_range(
    file: &fs::File,
    entry: &PackIndexEntry,
) -> Result<(), StoreError> {
    let end = entry
        .offset
        .checked_add(entry.length)
        .ok_or_else(|| StoreError::Format("pack index entry range overflows".to_string()))?;
    let pack_len = file.metadata()?.len();
    if end > pack_len {
        return Err(StoreError::Format(
            "pack index entry range exceeds pack size".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn pack_name_from_index_path(path: &Path) -> Result<String, StoreError> {
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return Err(StoreError::InvalidPath);
    };
    Ok(clean_pack_name(stem)?.to_string())
}

pub(crate) fn cleanup_pack_temp_files(pack_dir: &Path, pack_name: &str) -> Result<(), StoreError> {
    let pack_tmp_prefix = format!("{pack_name}.pack.tmp-");
    let index_tmp_prefix = format!("{pack_name}.idx.tmp-");
    for entry in fs::read_dir(pack_dir)? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if file_name.starts_with(&pack_tmp_prefix) || file_name.starts_with(&index_tmp_prefix) {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(StoreError::Io(err.to_string())),
            }
        }
    }
    Ok(())
}

pub(crate) fn clean_pack_name(value: &str) -> Result<&str, StoreError> {
    let valid = !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if valid {
        Ok(value)
    } else {
        Err(StoreError::InvalidPath)
    }
}
