use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub enum PathContainmentError {
    AbsolutePathNotAllowed {
        path: PathBuf,
    },
    LexicalTraversal {
        path: PathBuf,
    },
    ManagedRootNotAllowed {
        root: PathBuf,
    },
    OutsideManagedRoot {
        root: PathBuf,
        path: PathBuf,
    },
    FileSystem {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl PathContainmentError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::AbsolutePathNotAllowed { .. } => "ABSOLUTE_PATH_NOT_ALLOWED",
            Self::LexicalTraversal { .. }
            | Self::ManagedRootNotAllowed { .. }
            | Self::OutsideManagedRoot { .. }
            | Self::FileSystem { .. } => "PATH_OUTSIDE_REPO",
        }
    }
}

impl fmt::Display for PathContainmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbsolutePathNotAllowed { path } => {
                write!(
                    formatter,
                    "absolute path is not allowed: {}",
                    path.display()
                )
            }
            Self::LexicalTraversal { path } => {
                write!(
                    formatter,
                    "path contains lexical traversal: {}",
                    path.display()
                )
            }
            Self::ManagedRootNotAllowed { root } => {
                write!(
                    formatter,
                    "managed root itself is not a valid target: {}",
                    root.display()
                )
            }
            Self::OutsideManagedRoot { root, path } => write!(
                formatter,
                "path {} resolves outside managed root {}",
                path.display(),
                root.display()
            ),
            Self::FileSystem { path, source } => {
                write!(
                    formatter,
                    "failed to resolve path {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for PathContainmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FileSystem { source, .. } => Some(source),
            Self::AbsolutePathNotAllowed { .. }
            | Self::LexicalTraversal { .. }
            | Self::ManagedRootNotAllowed { .. }
            | Self::OutsideManagedRoot { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedManagedPath {
    managed_root: PathBuf,
    canonical_root: PathBuf,
    relative_path: PathBuf,
}

impl ValidatedManagedPath {
    pub fn validate(
        managed_root: &Path,
        relative_path: &Path,
    ) -> Result<Self, PathContainmentError> {
        let canonical_root = canonicalize_managed_root(managed_root)?;
        resolve_path_within_root(managed_root, relative_path, &canonical_root)?;
        Ok(Self {
            managed_root: managed_root.to_path_buf(),
            canonical_root,
            relative_path: relative_path.to_path_buf(),
        })
    }

    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Revalidates containment immediately before giving the target to an operation.
    ///
    /// Mutating callers intentionally cannot obtain a stored absolute target from this type.
    /// Hooks and other untrusted work should run before this method, so a changed symlink is
    /// detected at the final mutation boundary.
    pub fn with_revalidated_path<T, E>(
        &self,
        operation: impl FnOnce(&Path) -> Result<T, E>,
    ) -> Result<T, ValidatedPathOperationError<E>> {
        let current_root = canonicalize_managed_root(&self.managed_root)
            .map_err(ValidatedPathOperationError::Containment)?;
        if current_root != self.canonical_root {
            return Err(ValidatedPathOperationError::Containment(
                PathContainmentError::OutsideManagedRoot {
                    root: self.canonical_root.clone(),
                    path: current_root,
                },
            ));
        }
        let resolved = resolve_path_within_root(
            &self.managed_root,
            &self.relative_path,
            &self.canonical_root,
        )
        .map_err(ValidatedPathOperationError::Containment)?;
        operation(&resolved).map_err(ValidatedPathOperationError::Operation)
    }
}

#[derive(Debug)]
pub enum ValidatedPathOperationError<E> {
    Containment(PathContainmentError),
    Operation(E),
}

impl<E> fmt::Display for ValidatedPathOperationError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Containment(error) => error.fmt(formatter),
            Self::Operation(error) => error.fmt(formatter),
        }
    }
}

impl<E> std::error::Error for ValidatedPathOperationError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Containment(error) => Some(error),
            Self::Operation(error) => Some(error),
        }
    }
}

fn resolve_path_within_root(
    managed_root: &Path,
    relative_path: &Path,
    canonical_root: &Path,
) -> Result<PathBuf, PathContainmentError> {
    validate_relative_path(managed_root, relative_path)?;

    let joined = managed_root.join(relative_path);
    let resolved = resolve_using_existing_ancestor(&joined)?;

    if !resolved.starts_with(canonical_root) || resolved == canonical_root {
        return Err(PathContainmentError::OutsideManagedRoot {
            root: canonical_root.to_path_buf(),
            path: resolved,
        });
    }

    Ok(resolved)
}

fn canonicalize_managed_root(managed_root: &Path) -> Result<PathBuf, PathContainmentError> {
    managed_root
        .canonicalize()
        .map_err(|source| PathContainmentError::FileSystem {
            path: managed_root.to_path_buf(),
            source,
        })
}

fn validate_relative_path(
    managed_root: &Path,
    relative_path: &Path,
) -> Result<(), PathContainmentError> {
    if relative_path.is_absolute() {
        return Err(PathContainmentError::AbsolutePathNotAllowed {
            path: relative_path.to_path_buf(),
        });
    }

    let mut contains_name = false;
    for component in relative_path.components() {
        match component {
            Component::Normal(_) => contains_name = true,
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PathContainmentError::LexicalTraversal {
                    path: relative_path.to_path_buf(),
                });
            }
        }
    }
    if !contains_name {
        return Err(PathContainmentError::ManagedRootNotAllowed {
            root: managed_root.to_path_buf(),
        });
    }
    Ok(())
}

fn resolve_using_existing_ancestor(path: &Path) -> Result<PathBuf, PathContainmentError> {
    let mut ancestor = path.to_path_buf();
    let mut missing_components = Vec::<OsString>::new();

    loop {
        match ancestor.canonicalize() {
            Ok(mut canonical_ancestor) => {
                for component in missing_components.iter().rev() {
                    canonical_ancestor.push(component);
                }
                return Ok(canonical_ancestor);
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                let Some(file_name) = ancestor.file_name().map(OsString::from) else {
                    return Err(PathContainmentError::FileSystem {
                        path: ancestor,
                        source,
                    });
                };
                missing_components.push(file_name);
                if !ancestor.pop() {
                    return Err(PathContainmentError::FileSystem {
                        path: ancestor,
                        source,
                    });
                }
            }
            Err(source) => {
                return Err(PathContainmentError::FileSystem {
                    path: ancestor,
                    source,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::{PathContainmentError, ValidatedManagedPath, ValidatedPathOperationError};

    #[test]
    fn resolves_nonexistent_target_from_longest_existing_ancestor() {
        let fixture = tempdir().expect("create temporary directory");
        let root = fixture.path().join("managed");
        fs::create_dir(&root).expect("create managed root");

        let validated = ValidatedManagedPath::validate(&root, Path::new("new/nested/worktree"))
            .expect("validate target");
        let resolved = validated
            .with_revalidated_path(|path| Ok::<_, Infallible>(path.to_path_buf()))
            .expect("revalidate target");

        assert_eq!(
            resolved,
            root.canonicalize()
                .expect("canonicalize root")
                .join("new/nested/worktree")
        );
    }

    #[test]
    fn rejects_absolute_traversal_and_managed_root_paths() {
        let fixture = tempdir().expect("create temporary directory");
        let root = fixture.path().join("managed");
        fs::create_dir(&root).expect("create managed root");

        assert!(matches!(
            ValidatedManagedPath::validate(&root, fixture.path()),
            Err(PathContainmentError::AbsolutePathNotAllowed { .. })
        ));
        assert!(matches!(
            ValidatedManagedPath::validate(&root, Path::new("child/../sibling")),
            Err(PathContainmentError::LexicalTraversal { .. })
        ));
        assert!(matches!(
            ValidatedManagedPath::validate(&root, Path::new(".")),
            Err(PathContainmentError::ManagedRootNotAllowed { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_existing_and_nonexistent_targets_through_escaping_symlink() {
        use std::os::unix::fs::symlink;

        let fixture = tempdir().expect("create temporary directory");
        let root = fixture.path().join("managed");
        let outside = fixture.path().join("outside");
        fs::create_dir(&root).expect("create managed root");
        fs::create_dir(&outside).expect("create outside directory");
        fs::write(outside.join("existing"), "data").expect("create outside file");
        symlink(&outside, root.join("escape")).expect("create escaping symlink");

        for relative in ["escape/existing", "escape/not-created-yet"] {
            assert!(matches!(
                ValidatedManagedPath::validate(&root, Path::new(relative)),
                Err(PathContainmentError::OutsideManagedRoot { .. })
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn revalidation_rejects_a_symlink_swapped_after_pre_hook_validation() {
        use std::os::unix::fs::symlink;

        let fixture = tempdir().expect("create temporary directory");
        let root = fixture.path().join("managed");
        let outside = fixture.path().join("outside");
        let parent = root.join("parent");
        fs::create_dir_all(&parent).expect("create managed parent");
        fs::create_dir(&outside).expect("create outside directory");

        let validated = ValidatedManagedPath::validate(&root, Path::new("parent/target"))
            .expect("validate target before hook");

        fs::remove_dir(&parent).expect("simulate hook removing managed parent");
        symlink(&outside, &parent).expect("simulate hook swapping in an escaping symlink");

        let mut operation_called = false;
        let result = validated.with_revalidated_path(|_| {
            operation_called = true;
            Ok::<_, Infallible>(())
        });

        assert!(matches!(
            result,
            Err(ValidatedPathOperationError::Containment(
                PathContainmentError::OutsideManagedRoot { .. }
            ))
        ));
        assert!(!operation_called);
    }

    #[cfg(unix)]
    #[test]
    fn revalidation_rejects_the_managed_root_itself_being_swapped() {
        use std::os::unix::fs::symlink;

        let fixture = tempdir().expect("create temporary directory");
        let root = fixture.path().join("managed");
        let original_root = fixture.path().join("managed-before-swap");
        let outside = fixture.path().join("outside");
        fs::create_dir(&root).expect("create managed root");
        fs::create_dir(&outside).expect("create outside directory");

        let validated = ValidatedManagedPath::validate(&root, Path::new("target"))
            .expect("validate target before hook");

        fs::rename(&root, &original_root).expect("move original managed root");
        symlink(&outside, &root).expect("replace managed root with an escaping symlink");

        let result = validated.with_revalidated_path(|_| Ok::<_, Infallible>(()));

        assert!(matches!(
            result,
            Err(ValidatedPathOperationError::Containment(
                PathContainmentError::OutsideManagedRoot { .. }
            ))
        ));
    }
}
