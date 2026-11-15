//! MLX models: a directory of pinned files under the models folder, each
//! downloaded, verified, and receipted exactly like a GGUF file.
use super::*;

/// An MLX model: pinned files in one directory under the models folder.
/// Each file is a [`GgufArtifact`] in all but name (repository, revision,
/// size, SHA-256), so it downloads, verifies, and keeps its receipt exactly
/// as a GGUF file does.
pub struct MlxArtifact {
    pub directory: &'static str,
    pub files: &'static [GgufArtifact],
}

impl MlxArtifact {
    pub const fn bytes(&self) -> u64 {
        let mut total = 0;
        let mut index = 0;
        while index < self.files.len() {
            total += self.files[index].bytes;
            index += 1;
        }
        total
    }
}

impl ModelRuntime {
    /// Whether Hex downloads and verifies this model's files (GGUF or MLX),
    /// rather than the system managing it.
    pub const fn is_downloaded(self) -> bool {
        matches!(self, Self::Gguf(_) | Self::Mlx(_))
    }
}

/// Every file is present at its pinned size.
pub(super) fn installed(directory: &Path, artifact: &MlxArtifact) -> bool {
    artifact.files.iter().all(|file| {
        fs::metadata(directory.join(file.filename))
            .is_ok_and(|metadata| metadata.len() == file.bytes)
    })
}

/// Every file's receipt still matches its pin.
pub(super) fn verified(directory: &Path, artifact: &MlxArtifact) -> bool {
    artifact
        .files
        .iter()
        .all(|file| artifact_receipt_matches(&directory.join(file.filename), file))
}

/// Checks every file of an installed MLX model against its pin, hashing
/// only those whose receipt no longer matches.
pub(super) fn verify_installed_at(
    directory: &Path,
    artifact: &MlxArtifact,
    name: &str,
    canceled: &AtomicBool,
) -> Result<()> {
    check_canceled(canceled)?;
    if !directory.is_dir() {
        bail!("{name} is no longer installed. Open Settings to download it.");
    }
    let _lock = lock_downloads(directory, canceled)?;
    for file in artifact.files {
        let path = directory.join(file.filename);
        if !path.is_file() {
            bail!(
                "{name} is missing {}. Open Settings to download it again.",
                file.filename
            );
        }
        if !artifact_receipt_matches(&path, file) {
            verify_artifact(&path, file, name, canceled)?;
            write_artifact_receipt(&path, file)?;
        }
    }
    Ok(())
}

/// Deletes an MLX model's files, receipts, and partial downloads, then its
/// directory once nothing else is in it.
pub(super) fn remove_installed_at(directory: &Path, artifact: &MlxArtifact) -> Result<()> {
    for file in artifact.files {
        super::remove_installed_at(&directory.join(file.filename))?;
    }
    if directory.is_dir() {
        let _ = fs::remove_file(directory.join(".download.lock"));
        // Refuses while anything unexpected is left, which then stays.
        let _ = fs::remove_dir(directory);
        if let Some(parent) = directory.parent() {
            sync_directory(parent)?;
        }
    }
    Ok(())
}

/// Downloads every file of an MLX model into its directory, skipping
/// verified copies.
pub(super) fn download(
    model: &ModelDefinition,
    canceled: &AtomicBool,
    downloaded_bytes: &AtomicU64,
    stage: &AtomicU8,
) -> Result<PathBuf> {
    let ModelRuntime::Mlx(artifact) = model.runtime else {
        bail!("{} is not an MLX model", model.name);
    };
    let directory = model_path(model)?;
    for file in artifact.files {
        download_artifact_in(
            &directory,
            file,
            model.name,
            canceled,
            downloaded_bytes,
            stage,
        )?;
    }
    downloaded_bytes.store(artifact.bytes(), Ordering::Relaxed);
    if !speech_detector_installed() {
        crate::transcription::speech_detector::start_download();
    }
    Ok(directory)
}
