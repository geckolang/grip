pub const PATH_MANIFEST_FILE: &str = "grip.toml";
pub const PATH_DEPENDENCIES: &str = "dependencies";
const PATH_SOURCE_FILE_EXTENSION: &str = "ko";
const PATH_PACKAGE_LOCK: &str = "grip.lock";

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
pub enum PackageType {
  #[serde(rename = "library")]
  Library,
  #[serde(rename = "executable")]
  Executable,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct Manifest {
  pub name: String,
  #[serde(rename = "type")]
  pub ty: PackageType,
  pub version: String,
  pub dependencies: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PackageLock {
  pub built_dependencies: Vec<String>,
}

// TODO: Make use of return value.
// TODO: Pass in sub-command matches instead.
pub fn init_manifest(matches: &clap::ArgMatches<'_>) -> bool {
  let manifest_file_path = std::path::Path::new(PATH_MANIFEST_FILE);

  if manifest_file_path.exists() && !matches.is_present(crate::ARG_INIT_FORCE) {
    log::error!("manifest file already exists in this directory");

    return false;
  }

  if std::fs::create_dir(crate::PATH_SOURCES).is_err() {
    log::error!("failed to create sources directory");

    return false;
  }

  let default_manifest = toml::ser::to_string_pretty(&Manifest {
    name: String::from(matches.value_of(crate::ARG_INIT_NAME).unwrap()),
    ty: PackageType::Executable,
    version: String::from("0.0.1"),
    dependencies: Vec::new(),
  });

  if let Err(error) = default_manifest {
    log::error!("failed to stringify default package manifest: {}", error);

    return false;
  } else if let Err(error) = std::fs::write(manifest_file_path, default_manifest.unwrap()) {
    log::error!("failed to write default package manifest file: {}", error);

    return false;
  } else if let Err(error) = std::fs::write(
    std::path::PathBuf::from(".gitignore"),
    format!(
      "{}/\n{}/",
      crate::DEFAULT_OUTPUT_DIR,
      crate::PATH_DEPENDENCIES
    ),
  ) {
    log::error!("failed to write `.gitignore` file: {}", error);

    return false;
  }

  true
}

pub fn get_or_init_package_lock() -> Result<PackageLock, String> {
  let package_lock_path = std::path::Path::new(PATH_PACKAGE_LOCK);

  if !package_lock_path.exists() {
    let default_package_lock = toml::ser::to_string_pretty(&PackageLock {
      built_dependencies: Vec::new(),
    });

    if let Err(error) = default_package_lock {
      return Err(format!(
        "failed to stringify default package lock: {}",
        error
      ));
    } else if let Err(error) = std::fs::write(PATH_PACKAGE_LOCK, default_package_lock.unwrap()) {
      return Err(format!(
        "failed to write default package manifest file: {}",
        error
      ));
    }
  }

  let package_lock_contents = fetch_file_contents(&std::path::PathBuf::from(PATH_PACKAGE_LOCK))?;

  if let Ok(package_lock) = toml::from_str(&package_lock_contents) {
    Ok(package_lock)
  } else {
    Err("failed to parse package lock".to_string())
  }
}

pub fn fetch_file_contents(file_path: &std::path::PathBuf) -> Result<String, String> {
  if !file_path.is_file() {
    return Err(String::from(
      "path does not exist, is not a file, or is inaccessible",
    ));
  }

  let read_result = std::fs::read_to_string(file_path);

  if read_result.is_err() {
    return Err(String::from(
      "path does not exist or its contents are not valid utf-8",
    ));
  }

  Ok(read_result.unwrap())
}

pub fn fetch_manifest(path: &std::path::PathBuf) -> Result<Manifest, String> {
  let manifest_read_result = std::fs::read_to_string(path);

  if let Err(error) = manifest_read_result {
    return Err(format!("failed to read package manifest file: {}", error));
  }

  let manifest_result = toml::from_str::<Manifest>(manifest_read_result.unwrap().as_str());

  if let Err(error) = manifest_result {
    return Err(format!("failed to parse package manifest file: {}", error));
  }

  Ok(manifest_result.unwrap())
}

pub fn fetch_dependency_manifest(name: &str) -> Result<Manifest, String> {
  let dependency_manifest_path = std::path::PathBuf::from(PATH_DEPENDENCIES)
    .join(name)
    .join(PATH_MANIFEST_FILE);

  fetch_manifest(&dependency_manifest_path)
}

pub fn read_sources_dir(
  sources_dir: &std::path::PathBuf,
) -> Result<Vec<std::path::PathBuf>, String> {
  let read_dir_result = std::fs::read_dir(sources_dir);

  if let Err(error) = read_dir_result {
    return Err(format!("failed to read sources directory: {}", error));
  }

  let files = read_dir_result
    .unwrap()
    .map(|path_result| path_result.unwrap().path())
    .filter(|path| {
      if !path.is_file() {
        return false;
      }

      let extension = path.extension();

      extension.is_some() && extension.unwrap() == PATH_SOURCE_FILE_EXTENSION
    })
    .collect::<Vec<std::path::PathBuf>>()
    .into();

  Ok(files)
}
