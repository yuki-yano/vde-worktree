#[cfg(unix)]
use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::fd::{AsFd, OwnedFd};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::time::{SystemTime, UNIX_EPOCH};

use clap_complete::{Shell, generate};
#[cfg(unix)]
use nix::dir::Dir;
#[cfg(unix)]
use nix::fcntl::{AtFlags, OFlag, open, openat, renameat};
#[cfg(unix)]
use nix::sys::stat::{FileStat, Mode, fstat, fstatat, mkdirat};
#[cfg(unix)]
use nix::unistd::{UnlinkatFlags, fsync, unlinkat};
#[cfg(all(
    unix,
    any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "redox"
    )
))]
use rustix::fs::{RenameFlags, renameat_with};
use serde_json::json;

use crate::app::misc_commands::MiscCommandOutput;
use crate::cli::{Command, CompletionShell, ParsedRequest, clap_command};
use crate::domain::error::{CliError, ErrorCode};

pub fn execute_completion(
    request: &ParsedRequest,
    home: Option<&Path>,
) -> Option<Result<MiscCommandOutput, CliError>> {
    let Command::Completion {
        shell,
        install,
        path,
    } = &request.command
    else {
        return None;
    };
    Some(run_completion(*shell, *install, path.as_deref(), home))
}

pub fn generate_completion(shell: CompletionShell) -> Result<String, CliError> {
    let mut command = clap_command();
    let mut bytes = Vec::new();
    generate(
        match shell {
            CompletionShell::Zsh => Shell::Zsh,
            CompletionShell::Fish => Shell::Fish,
        },
        &mut command,
        "vw",
        &mut bytes,
    );
    let generated = String::from_utf8(bytes).map_err(|error| {
        CliError::new(
            ErrorCode::InternalError,
            format!("completion generator produced invalid UTF-8: {error}"),
        )
    })?;
    let script = match shell {
        CompletionShell::Zsh => enhance_zsh(&generated),
        CompletionShell::Fish => enhance_fish(&generated),
    };
    debug_assert!(!script.contains("node"));
    debug_assert!(!script.contains("npm"));
    debug_assert!(!script.contains("pnpm"));
    Ok(script)
}

fn run_completion(
    shell: CompletionShell,
    install: bool,
    requested_path: Option<&Path>,
    home: Option<&Path>,
) -> Result<MiscCommandOutput, CliError> {
    let script = generate_completion(shell)?;
    if !install {
        let mut output = MiscCommandOutput {
            data: json!({
                "shell": shell.as_str(),
                "installed": false,
                "script": script,
            }),
            human_stdout: script,
            human_stderr: String::new(),
            partial_error: None,
        };
        if !output.human_stdout.ends_with('\n') {
            output.human_stdout.push('\n');
        }
        return Ok(output);
    }
    let destination = requested_path.map_or_else(
        || default_install_path(shell, home),
        |path| Ok(path.to_path_buf()),
    )?;
    let cleanup_error = atomic_install(&destination, script.as_bytes())?;
    let installed = cleanup_error
        .as_ref()
        .and_then(|error| error.details.get("committed"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    let mut output = MiscCommandOutput {
        data: json!({
            "shell": shell.as_str(),
            "installed": installed,
            "path": destination,
        }),
        human_stdout: if installed {
            format!("installed completion: {}\n", destination.display())
        } else {
            format!(
                "completion installation requires recovery: {}\n",
                destination.display()
            )
        },
        human_stderr: String::new(),
        partial_error: None,
    };
    if installed && shell == CompletionShell::Zsh {
        output.human_stdout.push_str(
            "zsh note: add the directory to fpath, then run `autoload -Uz compinit && compinit`\n",
        );
    }
    output.partial_error = cleanup_error;
    Ok(output)
}

fn default_install_path(shell: CompletionShell, home: Option<&Path>) -> Result<PathBuf, CliError> {
    let home = home.ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "home directory is unavailable; pass --path explicitly",
        )
    })?;
    Ok(match shell {
        CompletionShell::Zsh => home.join(".zsh/completions/_vw"),
        CompletionShell::Fish => home.join(".config/fish/completions/vw.fish"),
    })
}

fn atomic_install(destination: &Path, content: &[u8]) -> Result<Option<CliError>, CliError> {
    #[cfg(unix)]
    return atomic_install_with_sync(destination, content, &sync_directory_handle);
    #[cfg(not(unix))]
    {
        let _ = (destination, content);
        Err(CliError::new(
            ErrorCode::UnsupportedRepositoryLayout,
            "completion installation is unsupported on this platform",
        ))
    }
}

#[cfg(unix)]
static COMPLETION_TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionInitializationPoint {
    DirectoryCreated,
    DirectoryOpened,
    IdentityVerified,
}

#[cfg(unix)]
trait CompletionInitializationObserver {
    fn checkpoint(&self, _point: CompletionInitializationPoint) -> Result<(), CliError> {
        Ok(())
    }
}

#[cfg(unix)]
struct NoopCompletionInitializationObserver;

#[cfg(unix)]
impl CompletionInitializationObserver for NoopCompletionInitializationObserver {}

#[cfg(unix)]
struct CompletionInitializationGuard {
    parent_fd: Option<OwnedFd>,
    name: std::ffi::OsString,
    directory_fd: Option<OwnedFd>,
    identity: Option<CompletionIdentity>,
    armed: bool,
}

#[cfg(unix)]
impl CompletionInitializationGuard {
    fn new(parent_fd: OwnedFd, name: std::ffi::OsString) -> Self {
        Self {
            parent_fd: Some(parent_fd),
            name,
            directory_fd: None,
            identity: None,
            armed: true,
        }
    }

    fn parent_fd(&self) -> &OwnedFd {
        self.parent_fd
            .as_ref()
            .expect("completion initialization parent exists")
    }

    fn set_directory_fd(&mut self, fd: OwnedFd) {
        self.directory_fd = Some(fd);
    }

    fn set_identity(&mut self, identity: CompletionIdentity) {
        self.identity = Some(identity);
    }

    fn cleanup(&self) -> Result<(), std::io::Error> {
        let named = fstatat(
            self.parent_fd(),
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(completion_identity_from_stat)
        .map_err(completion_errno_io)?;
        let expected = self.identity.unwrap_or(named);
        if named != expected {
            return Err(std::io::Error::other(
                "completion initialization entry changed; replacement was not removed",
            ));
        }
        let fallback_fd;
        let directory_fd = if let Some(directory_fd) = &self.directory_fd {
            directory_fd
        } else {
            fallback_fd = openat(
                self.parent_fd(),
                self.name.as_os_str(),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .map_err(completion_errno_io)?;
            &fallback_fd
        };
        let opened = fstat(directory_fd)
            .map(completion_identity_from_stat)
            .map_err(completion_errno_io)?;
        if opened != expected {
            return Err(std::io::Error::other(
                "opened completion initialization directory changed",
            ));
        }
        for name in ["new", "backup", "rolled-back"] {
            match unlinkat(directory_fd, name, UnlinkatFlags::NoRemoveDir) {
                Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
                Err(error) => return Err(completion_errno_io(error)),
            }
        }
        let named = fstatat(
            self.parent_fd(),
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(completion_identity_from_stat)
        .map_err(completion_errno_io)?;
        if named != expected {
            return Err(std::io::Error::other(
                "completion initialization entry changed during cleanup",
            ));
        }
        unlinkat(
            self.parent_fd(),
            self.name.as_os_str(),
            UnlinkatFlags::RemoveDir,
        )
        .map_err(completion_errno_io)
    }

    fn abort(mut self) -> Result<(), std::io::Error> {
        let result = self.cleanup();
        self.armed = false;
        result
    }

    fn finish(mut self) -> (OwnedFd, std::ffi::OsString, OwnedFd, CompletionIdentity) {
        self.armed = false;
        (
            self.parent_fd
                .take()
                .expect("completion initialization parent exists"),
            self.name.clone(),
            self.directory_fd
                .take()
                .expect("completion initialization directory exists"),
            self.identity
                .take()
                .expect("completion initialization identity exists"),
        )
    }
}

#[cfg(unix)]
impl Drop for CompletionInitializationGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.cleanup();
        }
    }
}

#[cfg(unix)]
struct CompletionTransaction {
    name: std::ffi::OsString,
    parent_fd: OwnedFd,
    fd: OwnedFd,
    identity: CompletionIdentity,
    preserve: bool,
}

#[cfg(unix)]
impl CompletionTransaction {
    fn create(parent_fd: &OwnedFd, display_parent: &Path) -> Result<Self, CliError> {
        Self::create_with_observer(
            parent_fd,
            display_parent,
            &NoopCompletionInitializationObserver,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn create_with_observer(
        parent_fd: &OwnedFd,
        display_parent: &Path,
        observer: &dyn CompletionInitializationObserver,
    ) -> Result<Self, CliError> {
        let transaction_parent_fd = openat(
            parent_fd,
            Path::new("."),
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| {
            completion_nix("duplicate completion parent handle", display_parent, error)
        })?;
        for _ in 0..128 {
            let counter = COMPLETION_TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let name = std::ffi::OsString::from(format!(
                ".vde-completion-{:x}-{nanos:x}-{counter:x}",
                std::process::id()
            ));
            match mkdirat(parent_fd, name.as_os_str(), Mode::from_bits_truncate(0o700)) {
                Ok(()) => {
                    let mut guard = CompletionInitializationGuard::new(transaction_parent_fd, name);
                    let initialization = (|| {
                        let identity = fstatat(
                            guard.parent_fd(),
                            guard.name.as_os_str(),
                            AtFlags::AT_SYMLINK_NOFOLLOW,
                        )
                        .map(completion_identity_from_stat)
                        .map_err(|error| {
                            completion_nix(
                                "inspect created completion transaction",
                                display_parent,
                                error,
                            )
                        })?;
                        guard.set_identity(identity);
                        observer.checkpoint(CompletionInitializationPoint::DirectoryCreated)?;
                        let fd = openat(
                            guard.parent_fd(),
                            guard.name.as_os_str(),
                            OFlag::O_RDONLY
                                | OFlag::O_DIRECTORY
                                | OFlag::O_NOFOLLOW
                                | OFlag::O_CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(|error| {
                            completion_nix(
                                "open completion transaction handle",
                                display_parent,
                                error,
                            )
                        })?;
                        guard.set_directory_fd(fd);
                        observer.checkpoint(CompletionInitializationPoint::DirectoryOpened)?;
                        let opened = completion_identity_fd(
                            guard
                                .directory_fd
                                .as_ref()
                                .expect("completion initialization directory exists"),
                            display_parent,
                        )?;
                        if Some(opened) != guard.identity {
                            return Err(completion_path_changed(
                                "completion transaction changed while its handle was opened",
                                display_parent,
                            ));
                        }
                        observer.checkpoint(CompletionInitializationPoint::IdentityVerified)?;
                        Ok::<(), CliError>(())
                    })();
                    if let Err(mut error) = initialization {
                        if let Err(cleanup) = guard.abort() {
                            error
                                .details
                                .insert("transactionCleanupFailed".to_owned(), json!(true));
                            error.details.insert(
                                "transactionCleanupError".to_owned(),
                                json!(cleanup.to_string()),
                            );
                        }
                        return Err(error);
                    }
                    let (parent_fd, name, fd, identity) = guard.finish();
                    return Ok(Self {
                        name,
                        parent_fd,
                        fd,
                        identity,
                        preserve: false,
                    });
                }
                Err(nix::errno::Errno::EEXIST) => {}
                Err(error) => {
                    return Err(completion_nix(
                        "create completion transaction through held parent",
                        display_parent,
                        error,
                    ));
                }
            }
        }
        Err(CliError::new(
            ErrorCode::InternalError,
            "failed to allocate a unique completion transaction",
        ))
    }

    fn display_path(&self, parent: &Path) -> PathBuf {
        parent.join(&self.name)
    }

    fn resolve_path(
        &self,
        parent: &Path,
        parent_identity: &CompletionIdentity,
    ) -> Result<PathBuf, CliError> {
        validate_completion_parent(parent, &self.parent_fd, parent_identity)?;
        let name = completion_find_entry_name_by_identity(&self.parent_fd, &self.identity)?
            .ok_or_else(|| {
                completion_path_changed(
                    "completion transaction is no longer reachable from its parent",
                    parent,
                )
            })?;
        Ok(parent.join(name))
    }

    fn validate_name(&self) -> Result<(), std::io::Error> {
        let named = fstatat(
            &self.parent_fd,
            self.name.as_os_str(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map(completion_identity_from_stat)
        .map_err(completion_errno_io)?;
        if named == self.identity {
            Ok(())
        } else {
            Err(std::io::Error::other(
                "completion transaction name changed; replacement was not touched",
            ))
        }
    }

    fn cleanup(&self) -> Result<(), std::io::Error> {
        self.validate_name()?;
        for name in ["new", "backup", "rolled-back"] {
            match unlinkat(&self.fd, name, UnlinkatFlags::NoRemoveDir) {
                Ok(()) | Err(nix::errno::Errno::ENOENT) => {}
                Err(error) => return Err(completion_errno_io(error)),
            }
        }
        self.validate_name()?;
        unlinkat(
            &self.parent_fd,
            self.name.as_os_str(),
            UnlinkatFlags::RemoveDir,
        )
        .map_err(completion_errno_io)
    }

    fn close(mut self) -> Result<(), std::io::Error> {
        let result = self.cleanup();
        self.preserve = true;
        result
    }

    fn keep(
        mut self,
        parent: &Path,
        parent_identity: &CompletionIdentity,
    ) -> Result<PathBuf, CliError> {
        let result = self.resolve_path(parent, parent_identity);
        self.preserve = true;
        result
    }
}

#[cfg(unix)]
impl Drop for CompletionTransaction {
    fn drop(&mut self) {
        if !self.preserve {
            let _ = self.cleanup();
        }
    }
}

#[cfg(unix)]
fn atomic_install_with_sync(
    destination: &Path,
    content: &[u8],
    sync: &dyn Fn(&OwnedFd, &Path) -> Result<(), CliError>,
) -> Result<Option<CliError>, CliError> {
    atomic_install_with_sync_and_observer(destination, content, sync, &NoopCompletionObserver)
}

#[cfg(unix)]
trait CompletionObserver {
    fn before_backup(&self, _destination: &Path) {}
    fn before_install(&self, _destination: &Path) {}
    fn after_backup(&self, _destination: &Path) {}
    fn before_transaction_cleanup(&self, _parent: &Path) {}
}

#[cfg(unix)]
struct NoopCompletionObserver;

#[cfg(unix)]
impl CompletionObserver for NoopCompletionObserver {}

#[cfg(unix)]
#[allow(clippy::too_many_lines)]
fn atomic_install_with_sync_and_observer(
    destination: &Path,
    content: &[u8],
    sync: &dyn Fn(&OwnedFd, &Path) -> Result<(), CliError>,
    observer: &dyn CompletionObserver,
) -> Result<Option<CliError>, CliError> {
    let parent = destination.parent().ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "completion installation path must have a parent directory",
        )
    })?;
    let name = destination.file_name().ok_or_else(|| {
        CliError::new(
            ErrorCode::InvalidArgument,
            "completion installation path must name a file",
        )
    })?;
    let (parent, parent_fd) = secure_completion_parent(parent)?;
    let parent_identity = completion_identity_fd(&parent_fd, &parent)?;
    validate_completion_parent(&parent, &parent_fd, &parent_identity)?;
    let transaction = CompletionTransaction::create(&parent_fd, &parent)?;
    let temporary_fd = openat(
        &transaction.fd,
        "new",
        OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(0o644),
    )
    .map_err(|error| {
        completion_nix(
            "create temporary file through transaction handle",
            &parent,
            error,
        )
    })?;
    let mut file = fs::File::from(temporary_fd);
    file.write_all(content).map_err(|error| {
        completion_io(
            "write temporary file",
            &transaction.display_path(&parent),
            &error,
        )
    })?;
    file.sync_all().map_err(|error| {
        completion_io(
            "sync temporary file",
            &transaction.display_path(&parent),
            &error,
        )
    })?;
    drop(file);

    let staged_identity = completion_identity_at(&transaction.fd, std::ffi::OsStr::new("new"))?
        .ok_or_else(|| completion_path_changed("temporary completion disappeared", destination))?;
    let existing_identity = completion_identity_at(&parent_fd, name)?;
    let existing = existing_identity.is_some();
    if existing {
        observer.before_backup(destination);
        validate_completion_parent(&parent, &parent_fd, &parent_identity)?;
        renameat(&parent_fd, name, &transaction.fd, "backup")
            .map_err(|error| completion_nix("backup existing completion", destination, error))?;
        let moved = match completion_identity_at(&transaction.fd, std::ffi::OsStr::new("backup")) {
            Ok(moved) => moved,
            Err(error) => {
                return finish_failed_install(
                    error,
                    transaction,
                    &parent_fd,
                    &parent,
                    &parent_identity,
                    name,
                    true,
                    false,
                    None,
                    sync,
                );
            }
        };
        if moved.as_ref().is_none_or(|moved| {
            existing_identity
                .as_ref()
                .is_none_or(|existing| moved != existing)
        }) {
            return finish_failed_install(
                completion_path_changed(
                    "completion changed while it was moved into transaction backup",
                    destination,
                ),
                transaction,
                &parent_fd,
                &parent,
                &parent_identity,
                name,
                true,
                false,
                None,
                sync,
            );
        }
        observer.after_backup(destination);
    }
    observer.before_install(destination);
    if let Err(error) = validate_completion_parent(&parent, &parent_fd, &parent_identity) {
        return finish_failed_install(
            error,
            transaction,
            &parent_fd,
            &parent,
            &parent_identity,
            name,
            existing,
            false,
            None,
            sync,
        );
    }
    if let Err(error) = completion_rename_noreplace(
        &transaction.fd,
        std::ffi::OsStr::new("new"),
        &parent_fd,
        name,
        destination,
    ) {
        return finish_failed_install(
            error,
            transaction,
            &parent_fd,
            &parent,
            &parent_identity,
            name,
            existing,
            false,
            None,
            sync,
        );
    }
    let installed_identity = match completion_identity_at(&parent_fd, name) {
        Ok(identity) => identity,
        Err(error) => {
            return finish_failed_install(
                error,
                transaction,
                &parent_fd,
                &parent,
                &parent_identity,
                name,
                existing,
                true,
                Some(&staged_identity),
                sync,
            );
        }
    };
    if installed_identity.as_ref() != Some(&staged_identity) {
        return finish_failed_install(
            completion_path_changed(
                "completion changed immediately after atomic install",
                destination,
            ),
            transaction,
            &parent_fd,
            &parent,
            &parent_identity,
            name,
            existing,
            true,
            Some(&staged_identity),
            sync,
        );
    }
    if let Err(error) = sync(&parent_fd, &parent) {
        return finish_failed_install(
            error,
            transaction,
            &parent_fd,
            &parent,
            &parent_identity,
            name,
            existing,
            true,
            Some(&staged_identity),
            sync,
        );
    }

    if let Err(error) = validate_completion_parent(&parent, &parent_fd, &parent_identity) {
        return finish_failed_install(
            error,
            transaction,
            &parent_fd,
            &parent,
            &parent_identity,
            name,
            existing,
            true,
            Some(&staged_identity),
            sync,
        );
    }
    observer.before_transaction_cleanup(&parent);
    let cleanup_display_path = transaction.display_path(&parent);
    let transaction_path = transaction.resolve_path(&parent, &parent_identity);
    let cleanup = transaction.close();
    match (transaction_path, cleanup) {
        (Ok(_), Ok(())) => {}
        (Ok(transaction_path), Err(error)) => {
            return Ok(Some(committed_cleanup_error(
                &transaction_path,
                &error.to_string(),
            )));
        }
        (Err(path_error), Ok(())) => {
            return Ok(Some(
                CliError::new(
                    ErrorCode::PathOutsideRepo,
                    "completion installed, but its parent path changed before cleanup",
                )
                .with_details(BTreeMap::from([
                    ("committed".to_owned(), json!(true)),
                    ("pathError".to_owned(), json!(path_error.message)),
                ])),
            ));
        }
        (Err(path_error), Err(error)) => {
            return Ok(Some(
                CliError::new(
                    ErrorCode::InternalError,
                    "completion installed, but transaction cleanup failed and its path is unavailable",
                )
                .with_details(BTreeMap::from([
                    ("committed".to_owned(), json!(true)),
                    ("transactionPathUnavailable".to_owned(), json!(true)),
                    ("pathError".to_owned(), json!(path_error.message)),
                    ("cleanupError".to_owned(), json!(error.to_string())),
                ])),
            ));
        }
    }
    match sync(&parent_fd, &parent) {
        Ok(()) => match validate_completion_parent(&parent, &parent_fd, &parent_identity) {
            Ok(()) => Ok(None),
            Err(error) => Ok(Some(committed_cleanup_error(
                &cleanup_display_path,
                &format!("{:?}: {}", error.code, error.message),
            ))),
        },
        Err(error) => Ok(Some(committed_cleanup_error(
            &cleanup_display_path,
            &format!("{:?}: {}", error.code, error.message),
        ))),
    }
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn finish_failed_install(
    mut error: CliError,
    transaction: CompletionTransaction,
    parent_fd: &OwnedFd,
    parent: &Path,
    parent_identity: &CompletionIdentity,
    name: &std::ffi::OsStr,
    existing: bool,
    installed: bool,
    installed_identity: Option<&CompletionIdentity>,
    sync: &dyn Fn(&OwnedFd, &Path) -> Result<(), CliError>,
) -> Result<Option<CliError>, CliError> {
    let failures = rollback_install(
        &transaction.fd,
        parent_fd,
        parent,
        name,
        existing,
        installed,
        installed_identity,
        sync,
    );
    attach_install_rollback(&mut error, &failures);
    if failures.is_empty() {
        if let Err(cleanup) = transaction.close() {
            error
                .details
                .insert("transactionCleanupFailed".to_owned(), json!(true));
            error.details.insert(
                "transactionCleanupError".to_owned(),
                json!(cleanup.to_string()),
            );
        }
        return Err(error);
    }
    error
        .details
        .insert("recoveryRequired".to_owned(), json!(true));
    match transaction.keep(parent, parent_identity) {
        Ok(recovery_path) => {
            error
                .details
                .insert("recoveryPath".to_owned(), json!(&recovery_path));
            error
                .details
                .insert("backupPath".to_owned(), json!(recovery_path.join("backup")));
        }
        Err(recovery_error) => {
            error
                .details
                .insert("recoveryPathUnavailable".to_owned(), json!(true));
            error.details.insert(
                "recoveryPathError".to_owned(),
                json!(recovery_error.message),
            );
        }
    }
    error.details.insert("phase".to_owned(), json!("rollback"));
    error
        .details
        .insert("committedState".to_owned(), json!("recovery-required"));
    Ok(Some(error))
}

#[cfg(unix)]
fn committed_cleanup_error(transaction_path: &Path, cleanup_error: &str) -> CliError {
    CliError::new(
        ErrorCode::InternalError,
        "completion installed, but transaction cleanup durability failed",
    )
    .with_details(BTreeMap::from([
        ("committed".to_owned(), json!(true)),
        ("transactionPath".to_owned(), json!(transaction_path)),
        ("cleanupError".to_owned(), json!(cleanup_error)),
    ]))
}

#[cfg(unix)]
#[allow(clippy::too_many_arguments)]
fn rollback_install(
    transaction_fd: &OwnedFd,
    parent_fd: &OwnedFd,
    parent: &Path,
    name: &std::ffi::OsStr,
    existing: bool,
    installed: bool,
    installed_identity: Option<&CompletionIdentity>,
    sync: &dyn Fn(&OwnedFd, &Path) -> Result<(), CliError>,
) -> Vec<String> {
    let mut failures = Vec::new();
    if installed {
        let current = completion_identity_at(parent_fd, name);
        if current.as_ref().ok().and_then(Option::as_ref) != installed_identity {
            failures.push("installed completion changed before rollback".to_owned());
        } else if let Err(error) = renameat(parent_fd, name, transaction_fd, "rolled-back") {
            failures.push(format!(
                "preserve installed completion before rollback: {error}"
            ));
        } else {
            let moved = completion_identity_at(transaction_fd, std::ffi::OsStr::new("rolled-back"));
            if moved.as_ref().ok().and_then(Option::as_ref) != installed_identity {
                let restore = completion_rename_noreplace(
                    transaction_fd,
                    std::ffi::OsStr::new("rolled-back"),
                    parent_fd,
                    name,
                    &parent.join(name),
                );
                failures.push(format!(
                    "installed completion changed while preserving rollback; restore={}",
                    restore
                        .err()
                        .map_or_else(|| "completed".to_owned(), |error| error.message)
                ));
            }
        }
    }
    if existing
        && let Err(error) = completion_rename_noreplace(
            transaction_fd,
            std::ffi::OsStr::new("backup"),
            parent_fd,
            name,
            &parent.join(name),
        )
    {
        failures.push(format!("restore previous completion: {}", error.message));
    }
    if let Err(error) = sync(parent_fd, parent) {
        failures.push(format!(
            "sync rollback: {:?}: {}",
            error.code, error.message
        ));
    }
    failures
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CompletionIdentity {
    device: u64,
    inode: u64,
    mode: u32,
}

#[cfg(unix)]
fn completion_identity_at(
    parent: &impl AsFd,
    name: &std::ffi::OsStr,
) -> Result<Option<CompletionIdentity>, CliError> {
    match fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(completion_identity_from_stat(stat))),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(error) => Err(completion_nix(
            "inspect completion through directory handle",
            Path::new(name),
            error,
        )),
    }
}

#[cfg(unix)]
fn completion_identity_from_stat(stat: FileStat) -> CompletionIdentity {
    CompletionIdentity {
        device: stat.st_dev.try_into().unwrap_or_default(),
        inode: stat.st_ino,
        mode: u32::from(stat.st_mode),
    }
}

#[cfg(unix)]
fn completion_find_entry_name_by_identity(
    directory: &OwnedFd,
    identity: &CompletionIdentity,
) -> Result<Option<std::ffi::OsString>, CliError> {
    let duplicate = openat(
        directory,
        Path::new("."),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        completion_nix(
            "duplicate completion parent for scan",
            Path::new("."),
            error,
        )
    })?;
    let mut entries = Dir::from_fd(duplicate)
        .map_err(|error| completion_nix("scan completion parent", Path::new("."), error))?;
    for entry in entries.iter() {
        let entry = entry.map_err(|error| {
            completion_nix("read completion parent entry", Path::new("."), error)
        })?;
        let name = std::ffi::OsString::from_vec(entry.file_name().to_bytes().to_vec());
        if name == "." || name == ".." {
            continue;
        }
        let Some(candidate) = completion_identity_at(directory, name.as_os_str())? else {
            continue;
        };
        if candidate == *identity {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn completion_identity_fd(fd: &impl AsFd, path: &Path) -> Result<CompletionIdentity, CliError> {
    fstat(fd)
        .map(completion_identity_from_stat)
        .map_err(|error| completion_nix("inspect opened directory handle", path, error))
}

#[cfg(unix)]
fn validate_completion_parent(
    parent: &Path,
    parent_fd: &OwnedFd,
    expected: &CompletionIdentity,
) -> Result<(), CliError> {
    let metadata = fs::symlink_metadata(parent)
        .map_err(|error| completion_io("inspect completion parent path", parent, &error))?;
    let path_identity = CompletionIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
    };
    let held_identity = completion_identity_fd(parent_fd, parent)?;
    if path_identity != *expected || held_identity != *expected {
        return Err(completion_path_changed(
            "completion parent changed while installation was in progress",
            parent,
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn completion_errno_io(error: nix::errno::Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error as i32)
}

#[cfg(unix)]
fn completion_path_changed(message: &str, path: &Path) -> CliError {
    CliError::new(ErrorCode::PathOutsideRepo, message)
        .with_details(BTreeMap::from([("path".to_owned(), json!(path))]))
}

#[cfg(all(
    unix,
    any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "redox"
    )
))]
fn completion_rename_noreplace<OldFd: AsFd, NewFd: AsFd>(
    old_fd: &OldFd,
    old_name: &std::ffi::OsStr,
    new_fd: &NewFd,
    new_name: &std::ffi::OsStr,
    destination: &Path,
) -> Result<(), CliError> {
    renameat_with(old_fd, old_name, new_fd, new_name, RenameFlags::NOREPLACE).map_err(|error| {
        completion_io(
            "atomically install without replacing a concurrent completion",
            destination,
            &std::io::Error::from_raw_os_error(error.raw_os_error()),
        )
    })
}

#[cfg(all(
    unix,
    not(any(
        target_vendor = "apple",
        target_os = "linux",
        target_os = "android",
        target_os = "redox"
    ))
))]
fn completion_rename_noreplace<OldFd: AsFd, NewFd: AsFd>(
    _old_fd: &OldFd,
    _old_name: &std::ffi::OsStr,
    _new_fd: &NewFd,
    _new_name: &std::ffi::OsStr,
    _destination: &Path,
) -> Result<(), CliError> {
    Err(CliError::new(
        ErrorCode::UnsupportedRepositoryLayout,
        "atomic no-clobber completion installation is unsupported on this Unix platform",
    ))
}

#[cfg(unix)]
fn attach_install_rollback(error: &mut CliError, failures: &[String]) {
    error.details.insert("committed".to_owned(), json!(false));
    if !failures.is_empty() {
        error
            .details
            .insert("rollbackFailed".to_owned(), json!(true));
        error
            .details
            .insert("rollbackFailures".to_owned(), json!(failures));
    }
}

#[cfg(unix)]
fn sync_directory_handle(fd: &OwnedFd, path: &Path) -> Result<(), CliError> {
    fsync(fd).map_err(|error| completion_nix("sync completion directory handle", path, error))
}

#[cfg(unix)]
fn open_directory(path: &Path) -> Result<OwnedFd, CliError> {
    open(
        path,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| completion_nix("open completion directory", path, error))
}

#[cfg(unix)]
#[allow(clippy::too_many_lines)]
fn secure_completion_parent(parent: &Path) -> Result<(PathBuf, OwnedFd), CliError> {
    let absolute = if parent.is_absolute() {
        parent.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| completion_io("resolve current directory", parent, &error))?
            .join(parent)
    };
    if absolute
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(CliError::new(
            ErrorCode::PathOutsideRepo,
            "completion path must not contain parent traversal",
        ));
    }
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(existing) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(CliError::new(
                    ErrorCode::InvalidArgument,
                    format!(
                        "completion directory ancestor is not a directory: {}",
                        existing.display()
                    ),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or_else(|| {
                    CliError::new(
                        ErrorCode::InvalidArgument,
                        "completion path has no existing directory ancestor",
                    )
                })?;
                missing.push(name.to_os_string());
                existing = existing.parent().ok_or_else(|| {
                    CliError::new(
                        ErrorCode::InvalidArgument,
                        "completion path has no existing directory ancestor",
                    )
                })?;
            }
            Err(error) => {
                return Err(completion_io(
                    "inspect completion directory ancestor",
                    existing,
                    &error,
                ));
            }
        }
    }
    let mut resolved = fs::canonicalize(existing).map_err(|error| {
        completion_io("resolve completion directory ancestor", existing, &error)
    })?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    let mut current = open_directory(Path::new("/"))?;
    let mut traversed = PathBuf::from("/");
    for component in resolved.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        traversed.push(name);
        match openat(
            &current,
            name,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        ) {
            Ok(next) => current = next,
            Err(nix::errno::Errno::ENOENT) => {
                mkdirat(&current, name, Mode::from_bits_truncate(0o755)).map_err(|error| {
                    completion_nix(
                        "create completion directory through handle",
                        &traversed,
                        error,
                    )
                })?;
                current = openat(
                    &current,
                    name,
                    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|error| {
                    completion_nix("open created completion directory", &traversed, error)
                })?;
            }
            Err(error) => {
                return Err(completion_nix(
                    "open completion directory without following symlinks",
                    &traversed,
                    error,
                ));
            }
        }
    }
    Ok((resolved, current))
}

#[cfg(unix)]
fn completion_nix(action: &str, path: &Path, error: nix::errno::Errno) -> CliError {
    completion_io(
        action,
        path,
        &std::io::Error::from_raw_os_error(error as i32),
    )
}

#[cfg(unix)]
fn completion_io(action: &str, path: &Path, error: &std::io::Error) -> CliError {
    CliError::new(
        ErrorCode::InternalError,
        format!("failed to {action} at {}: {error}", path.display()),
    )
    .with_details(BTreeMap::from([
        ("path".to_owned(), json!(path)),
        ("cause".to_owned(), json!(error.to_string())),
    ]))
}

fn enhance_zsh(generated: &str) -> String {
    let generated = generated
        .replacen("#compdef vw", "#compdef vw vde-worktree", 1)
        .replace("'::branch:_default'", "'::branch:_vw_complete_worktrees'")
        .replace("':branch:_default'", "':branch:_vw_complete_worktrees'")
        .replace(
            "':remote_branch:_default'",
            "':remote_branch:_vw_complete_remote_branches'",
        )
        .replace("':hook:_default'", "':hook:_vw_complete_hooks'")
        .replace(
            "'--from=[]:FROM:_default'",
            "'--from=[]:FROM:_vw_complete_managed_worktrees'",
        )
        .replace(
            "'--to=[]:TO:_default'",
            "'--to=[]:TO:_vw_complete_managed_worktrees'",
        );
    let generated = replace_zsh_command_argument(
        &generated,
        "use",
        "':branch:_vw_complete_worktrees'",
        "':branch:_vw_complete_use_branches'",
    );
    let generated = replace_zsh_command_argument(
        &generated,
        "unabsorb",
        "':branch:_vw_complete_worktrees'",
        "':branch:_vw_complete_use_branches'",
    );
    let helpers = r#"
# Dynamic candidates are emitted as shell-safe TSV by the Rust binary.
_vw_dynamic_candidates() {
  local kind="$1" row value description
  local vw_bin="${words[1]:-vw}"
  local -a values
  command -v "$vw_bin" >/dev/null 2>&1 || return 0
  while IFS=$'\t' read -r value description; do
    [[ -n "$value" ]] || continue
    values+=("${value}:${description}")
  done < <(command "$vw_bin" __complete "$kind" 2>/dev/null)
  (( ${#values} > 0 )) && _describe -t "$kind" "$kind" values
}

_vw_complete_worktrees() { _vw_dynamic_candidates worktrees }
_vw_complete_use_branches() { _vw_dynamic_candidates use-branches }
_vw_complete_remote_branches() { _vw_dynamic_candidates remote-branches }
_vw_complete_hooks() { _vw_dynamic_candidates hooks }
_vw_complete_managed_worktrees() { _vw_dynamic_candidates managed-worktrees }
"#;
    generated.replacen('\n', &format!("\n{helpers}\n"), 1)
}

fn replace_zsh_command_argument(
    generated: &str,
    command: &str,
    needle: &str,
    replacement: &str,
) -> String {
    let marker = format!("({command})\n");
    let Some(start) = generated.find(&marker) else {
        return generated.to_owned();
    };
    let body_start = start + marker.len();
    let Some(relative_end) = generated[body_start..].find("\n;;") else {
        return generated.to_owned();
    };
    let end = body_start + relative_end;
    let mut output = String::with_capacity(generated.len());
    output.push_str(&generated[..body_start]);
    output.push_str(&generated[body_start..end].replace(needle, replacement));
    output.push_str(&generated[end..]);
    output
}

fn enhance_fish(generated: &str) -> String {
    let aliases = generated
        .lines()
        .filter(|line| line.starts_with("complete -c vw "))
        .map(|line| line.replacen("complete -c vw ", "complete -c vde-worktree ", 1))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r"{generated}
{aliases}

# Dynamic candidates are emitted as shell-safe TSV by the Rust binary.
function __vw_dynamic_candidates
    set -l tokens (commandline -opc)
    set -l vw_bin vw
    if test (count $tokens) -gt 0
        set vw_bin $tokens[1]
    end
    command $vw_bin __complete $argv 2>/dev/null
end

for __vw_bin in vw vde-worktree
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from status path switch del absorb exec lock unlock' -a '(__vw_dynamic_candidates worktrees)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from use unabsorb' -a '(__vw_dynamic_candidates use-branches)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from get' -a '(__vw_dynamic_candidates remote-branches)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from invoke' -a '(__vw_dynamic_candidates hooks)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from absorb' -l from -a '(__vw_dynamic_candidates managed-worktrees)'
    complete -c $__vw_bin -f -n '__fish_seen_subcommand_from unabsorb' -l to -a '(__vw_dynamic_candidates managed-worktrees)'
end
"
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn generated_scripts_are_node_free_and_include_both_binary_names() {
        for shell in [CompletionShell::Zsh, CompletionShell::Fish] {
            let script = generate_completion(shell).unwrap();
            assert!(!script.contains("node"));
            assert!(!script.contains("npm"));
            assert!(!script.contains("pnpm"));
            assert!(script.contains("vw"));
            assert!(script.contains("vde-worktree"));
            assert!(script.contains("__complete"));
        }
    }

    #[test]
    fn generated_scripts_wire_worktree_and_use_branch_candidate_kinds_separately() {
        let zsh = generate_completion(CompletionShell::Zsh).unwrap();
        assert!(
            zsh.contains("_vw_complete_use_branches() { _vw_dynamic_candidates use-branches }")
        );
        for command in ["use", "unabsorb"] {
            let marker = format!("({command})\n");
            let block = zsh
                .split_once(&marker)
                .unwrap()
                .1
                .split_once("\n;;")
                .unwrap()
                .0;
            assert!(block.contains("':branch:_vw_complete_use_branches'"));
            assert!(!block.contains("':branch:_vw_complete_worktrees'"));
        }
        let fish = generate_completion(CompletionShell::Fish).unwrap();
        assert!(fish.contains("__fish_seen_subcommand_from use unabsorb"));
        assert!(fish.contains("__vw_dynamic_candidates use-branches"));
    }

    #[test]
    fn atomic_install_replaces_existing_content_and_leaves_no_temporary_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("nested/_vw");
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(&destination, "old").unwrap();

        atomic_install(&destination, b"new\n").unwrap();

        assert_eq!(fs::read_to_string(&destination).unwrap(), "new\n");
        let entries = fs::read_dir(destination.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, [destination.file_name().unwrap()]);
    }

    #[test]
    fn directory_sync_failure_restores_previous_completion_before_returning_error() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        fs::write(&destination, "old\n").unwrap();
        let calls = Cell::new(0_u8);
        let sync = |_fd: &OwnedFd, _path: &Path| {
            let current = calls.get();
            calls.set(current + 1);
            if current == 0 {
                Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected directory sync failure",
                ))
            } else {
                Ok(())
            }
        };

        let error = atomic_install_with_sync(&destination, b"new\n", &sync).unwrap_err();

        assert_eq!(error.code, ErrorCode::InternalError);
        assert_eq!(error.details["committed"], false);
        assert_eq!(fs::read_to_string(&destination).unwrap(), "old\n");
        assert_eq!(calls.get(), 2);
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vde-completion-")
        }));
    }

    #[test]
    fn default_paths_follow_shell_conventions() {
        let home = Path::new("/home/example");
        assert_eq!(
            default_install_path(CompletionShell::Zsh, Some(home)).unwrap(),
            home.join(".zsh/completions/_vw")
        );
        assert_eq!(
            default_install_path(CompletionShell::Fish, Some(home)).unwrap(),
            home.join(".config/fish/completions/vw.fish")
        );
    }

    #[test]
    fn post_commit_cleanup_sync_failure_is_an_explicit_partial_error() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        fs::write(&destination, "old\n").unwrap();
        let calls = Cell::new(0_u8);
        let sync = |_fd: &OwnedFd, _path: &Path| {
            let current = calls.get();
            calls.set(current + 1);
            if current == 1 {
                Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected cleanup durability failure",
                ))
            } else {
                Ok(())
            }
        };

        let partial = atomic_install_with_sync(&destination, b"new\n", &sync)
            .unwrap()
            .expect("committed cleanup failure");

        assert_eq!(partial.code, ErrorCode::InternalError);
        assert_eq!(partial.details["committed"], true);
        assert_eq!(fs::read_to_string(&destination).unwrap(), "new\n");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn rollback_failure_keeps_previous_completion_in_a_recovery_transaction() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        fs::write(&destination, "old\n").unwrap();
        let calls = Cell::new(0_u8);
        let sync = |_fd: &OwnedFd, _path: &Path| {
            let current = calls.get();
            calls.set(current + 1);
            if current == 0 {
                fs::remove_file(&destination).unwrap();
                fs::create_dir(&destination).unwrap();
                Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected install durability failure",
                ))
            } else {
                Ok(())
            }
        };

        let partial = atomic_install_with_sync(&destination, b"new\n", &sync)
            .unwrap()
            .expect("rollback failure must be a partial recovery result");

        assert_eq!(partial.details["committed"], false);
        assert_eq!(partial.details["recoveryRequired"], true);
        assert_eq!(partial.details["rollbackFailed"], true);
        assert_eq!(partial.details["phase"], "rollback");
        let backup = PathBuf::from(partial.details["backupPath"].as_str().unwrap());
        assert_eq!(fs::read_to_string(backup).unwrap(), "old\n");
    }

    #[test]
    fn replacing_a_completion_symlink_never_mutates_its_outside_target() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        let outside = directory.path().join("outside");
        fs::write(&outside, "outside sentinel\n").unwrap();
        symlink(&outside, &destination).unwrap();

        atomic_install(&destination, b"new\n").unwrap();

        assert_eq!(fs::read_to_string(&destination).unwrap(), "new\n");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "outside sentinel\n");
        assert!(
            !fs::symlink_metadata(&destination)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    struct CreateCompletionBeforeInstall {
        destination: PathBuf,
    }

    impl CompletionObserver for CreateCompletionBeforeInstall {
        fn before_install(&self, _destination: &Path) {
            fs::write(&self.destination, "concurrent\n").unwrap();
        }
    }

    #[test]
    fn absent_completion_created_after_validation_is_never_clobbered() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        let sync = |_fd: &OwnedFd, _path: &Path| Ok(());

        let error = atomic_install_with_sync_and_observer(
            &destination,
            b"new\n",
            &sync,
            &CreateCompletionBeforeInstall {
                destination: destination.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::InternalError);
        assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent\n");
    }

    struct SwapCompletionBeforeBackup {
        destination: PathBuf,
        original: PathBuf,
    }

    impl CompletionObserver for SwapCompletionBeforeBackup {
        fn before_backup(&self, _destination: &Path) {
            fs::rename(&self.destination, &self.original).unwrap();
            fs::write(&self.destination, "concurrent\n").unwrap();
        }
    }

    #[test]
    fn completion_swapped_before_backup_is_detected_without_clobber() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        let original = directory.path().join("_vw-original");
        fs::write(&destination, "old\n").unwrap();
        let sync = |_fd: &OwnedFd, _path: &Path| Ok(());

        let error = atomic_install_with_sync_and_observer(
            &destination,
            b"new\n",
            &sync,
            &SwapCompletionBeforeBackup {
                destination: destination.clone(),
                original: original.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent\n");
        assert_eq!(fs::read_to_string(original).unwrap(), "old\n");
    }

    struct CreateCompletionAfterBackup {
        destination: PathBuf,
    }

    impl CompletionObserver for CreateCompletionAfterBackup {
        fn after_backup(&self, _destination: &Path) {
            fs::write(&self.destination, "concurrent\n").unwrap();
        }
    }

    #[test]
    fn completion_created_after_verified_backup_preserves_all_values_for_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        fs::write(&destination, "old\n").unwrap();
        let sync = |_fd: &OwnedFd, _path: &Path| Ok(());

        let partial = atomic_install_with_sync_and_observer(
            &destination,
            b"new\n",
            &sync,
            &CreateCompletionAfterBackup {
                destination: destination.clone(),
            },
        )
        .unwrap()
        .expect("restore collision must preserve a recovery transaction");

        assert_eq!(partial.details["recoveryRequired"], true);
        assert_eq!(fs::read_to_string(destination).unwrap(), "concurrent\n");
        let recovery = PathBuf::from(partial.details["recoveryPath"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(recovery.join("backup")).unwrap(),
            "old\n"
        );
        assert_eq!(fs::read_to_string(recovery.join("new")).unwrap(), "new\n");
    }

    struct SwapCompletionParentBeforeInstall {
        parent: PathBuf,
        original: PathBuf,
    }

    impl CompletionObserver for SwapCompletionParentBeforeInstall {
        fn before_install(&self, _destination: &Path) {
            fs::rename(&self.parent, &self.original).unwrap();
            fs::create_dir(&self.parent).unwrap();
            fs::write(self.parent.join("_vw"), "replacement sentinel\n").unwrap();
        }
    }

    #[test]
    fn completion_parent_swap_rolls_back_through_held_fd_without_touching_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("completions");
        let original = directory.path().join("completions-original");
        fs::create_dir(&parent).unwrap();
        let destination = parent.join("_vw");
        fs::write(&destination, "old\n").unwrap();
        let sync = |_fd: &OwnedFd, _path: &Path| Ok(());

        let error = atomic_install_with_sync_and_observer(
            &destination,
            b"new\n",
            &sync,
            &SwapCompletionParentBeforeInstall {
                parent: parent.clone(),
                original: original.clone(),
            },
        )
        .unwrap_err();

        assert_eq!(error.code, ErrorCode::PathOutsideRepo);
        assert_eq!(
            fs::read_to_string(parent.join("_vw")).unwrap(),
            "replacement sentinel\n"
        );
        assert_eq!(fs::read_to_string(original.join("_vw")).unwrap(), "old\n");
        assert!(fs::read_dir(&original).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".vde-completion-")
        }));
    }

    struct SwapCompletionTransactionBeforeCleanup;

    impl CompletionObserver for SwapCompletionTransactionBeforeCleanup {
        fn before_transaction_cleanup(&self, parent: &Path) {
            let transaction = fs::read_dir(parent)
                .unwrap()
                .map(Result::unwrap)
                .find(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".vde-completion-")
                })
                .unwrap()
                .path();
            fs::rename(&transaction, parent.join("completion-recovery")).unwrap();
            fs::create_dir(&transaction).unwrap();
            fs::write(transaction.join("replacement-sentinel"), "untouched\n").unwrap();
        }
    }

    #[test]
    fn completion_transaction_name_swap_preserves_replacement_and_reports_resolved_path() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("_vw");
        fs::write(&destination, "old\n").unwrap();
        let sync = |_fd: &OwnedFd, _path: &Path| Ok(());

        let partial = atomic_install_with_sync_and_observer(
            &destination,
            b"new\n",
            &sync,
            &SwapCompletionTransactionBeforeCleanup,
        )
        .unwrap()
        .expect("the renamed transaction must be reported for recovery");

        assert_eq!(partial.details["committed"], true);
        assert_eq!(
            partial.details["transactionPath"],
            json!(
                directory
                    .path()
                    .canonicalize()
                    .unwrap()
                    .join("completion-recovery")
            )
        );
        assert_eq!(fs::read_to_string(&destination).unwrap(), "new\n");
        let replacement = fs::read_dir(directory.path())
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vde-completion-")
            })
            .unwrap()
            .path();
        assert_eq!(
            fs::read_to_string(replacement.join("replacement-sentinel")).unwrap(),
            "untouched\n"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("completion-recovery/backup")).unwrap(),
            "old\n"
        );
    }

    struct FailCompletionInitializationAt(CompletionInitializationPoint);

    impl CompletionInitializationObserver for FailCompletionInitializationAt {
        fn checkpoint(&self, point: CompletionInitializationPoint) -> Result<(), CliError> {
            if point == self.0 {
                Err(CliError::new(
                    ErrorCode::InternalError,
                    format!("injected completion initialization failure at {point:?}"),
                ))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn every_completion_initialization_failure_removes_the_hidden_transaction() {
        for point in [
            CompletionInitializationPoint::DirectoryCreated,
            CompletionInitializationPoint::DirectoryOpened,
            CompletionInitializationPoint::IdentityVerified,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (parent, parent_fd) = secure_completion_parent(directory.path()).unwrap();

            let error = CompletionTransaction::create_with_observer(
                &parent_fd,
                &parent,
                &FailCompletionInitializationAt(point),
            )
            .err()
            .unwrap();

            assert_eq!(error.code, ErrorCode::InternalError, "point={point:?}");
            assert!(
                fs::read_dir(&parent).unwrap().all(|entry| {
                    !entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".vde-completion-")
                }),
                "point={point:?}"
            );
            assert_eq!(error.details.get("transactionCleanupFailed"), None);
        }
    }

    struct SwapCompletionInitializationEntry {
        parent: PathBuf,
    }

    impl CompletionInitializationObserver for SwapCompletionInitializationEntry {
        fn checkpoint(&self, point: CompletionInitializationPoint) -> Result<(), CliError> {
            if point == CompletionInitializationPoint::DirectoryCreated {
                let transaction = fs::read_dir(&self.parent)
                    .unwrap()
                    .map(Result::unwrap)
                    .find(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".vde-completion-")
                    })
                    .unwrap()
                    .path();
                fs::rename(&transaction, self.parent.join("original-transaction")).unwrap();
                fs::create_dir(&transaction).unwrap();
                fs::write(transaction.join("sentinel"), "replacement\n").unwrap();
                return Err(CliError::new(
                    ErrorCode::InternalError,
                    "injected replacement race",
                ));
            }
            Ok(())
        }
    }

    #[test]
    fn completion_initialization_cleanup_never_deletes_a_replacement_entry() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, parent_fd) = secure_completion_parent(directory.path()).unwrap();

        let error = CompletionTransaction::create_with_observer(
            &parent_fd,
            &parent,
            &SwapCompletionInitializationEntry {
                parent: parent.clone(),
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.details["transactionCleanupFailed"], true);
        let replacement = fs::read_dir(&parent)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".vde-completion-")
            })
            .unwrap()
            .path();
        assert_eq!(
            fs::read_to_string(replacement.join("sentinel")).unwrap(),
            "replacement\n"
        );
        assert!(parent.join("original-transaction").is_dir());
    }
}
