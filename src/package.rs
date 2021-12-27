pub const PATH_MANIFEST_FILE: &str = "grip.toml";
const PATH_SOURCE_FILE_EXTENSION: &str = "ko";

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PackageManifest {
  pub name: String,
  pub version: String,
}

// TODO: Make use of return value.
// TODO: Pass in sub-command matches instead.
pub fn init_manifest(matches: &clap::ArgMatches<'_>) -> bool {
  let manifest_file_path = std::path::Path::new(PATH_MANIFEST_FILE);

  if manifest_file_path.exists()
    && !matches
      .subcommand_matches(crate::ARG_INIT)
      .unwrap()
      .is_present(crate::ARG_INIT_FORCE)
  {
    log::error!("manifest file already exists in this directory");

    return false;
  }

  log::info!(
    "Creating package in {:?}",
    std::fs::canonicalize(".").unwrap()
  );

  if let Err(e) = std::fs::create_dir(crate::DEFAULT_SOURCES_DIR) {
    log::error!(
      "Failed to create sources directory {:?}",
      crate::DEFAULT_SOURCES_DIR
    );
    log::error!("{}", e);
    return false;
  }

  if let Err(e) = std::fs::create_dir(crate::DEFAULT_OUTPUT_DIR) {
    log::error!(
      "Failed to create output directory {:?}",
      crate::DEFAULT_OUTPUT_DIR
    );
    log::error!("{}", e);
    return false;
  }

  let default_package_manifest = toml::ser::to_string_pretty(&PackageManifest {
    name: String::from(
      matches
        .subcommand_matches(crate::ARG_INIT)
        .unwrap()
        .value_of(crate::ARG_INIT_NAME)
        .unwrap(),
    ),
    version: String::from("0.1.0"),
  });

  if let Err(error) = default_package_manifest {
    log::error!("failed to stringify default package manifest: {}", error);

    return false;
  } else if let Err(error) = std::fs::write(manifest_file_path, default_package_manifest.unwrap()) {
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

pub fn fetch_source_file_contents(source_file_path: &std::path::PathBuf) -> Result<String, String> {
  if !source_file_path.is_file() {
    return Err(String::from(
      "path does not exist, is not a file, or is inaccessible",
    ));
  }

  let source_file_contents = std::fs::read_to_string(source_file_path.clone());

  if source_file_contents.is_err() {
    return Err(String::from(
      "path does not exist or its contents are not valid UTF-8",
    ));
  }

  Ok(source_file_contents.unwrap())
}

pub fn read_manifest() -> Result<PackageManifest, String> {
  let manifest_file_contents_result = match std::fs::read_to_string(PATH_MANIFEST_FILE) {
    Err(error) => {
      log::error!(
        "Failed to read package manifest {:?}",
        std::fs::canonicalize(PATH_MANIFEST_FILE)
          .unwrap_or(std::path::PathBuf::from(PATH_MANIFEST_FILE))
      );
      log::error!("{}", error);
      return Err(format!("{}", error));
    }
    Ok(v) => v,
  };

  match toml::from_str::<PackageManifest>(manifest_file_contents_result.as_str()) {
    Err(error) => {
      log::error!(
        "Failed to parse package manifest {:?}",
        std::fs::canonicalize(PATH_MANIFEST_FILE)
          .unwrap_or(std::path::PathBuf::from(PATH_MANIFEST_FILE))
      );
      log::error!("{}", error);
      Err(format!("{}", error))
    }
    Ok(v) => Ok(v),
  }
}

pub fn read_sources_dir(
  sources_dir: &std::path::PathBuf,
) -> Result<Vec<std::path::PathBuf>, String> {
  match std::fs::read_dir(sources_dir) {
    Err(error) => {
      log::error!("Failed to read sources directory {:?}", sources_dir);
      log::error!("{}", error);
      Err(format!("{}", error))
    }
    Ok(result) => Ok(
      result
        .map(|path_result| path_result.unwrap().path())
        .filter(|path| {
          if !path.is_file() {
            return false;
          }

          let extension = path.extension();

          extension.is_some() && extension.unwrap() == PATH_SOURCE_FILE_EXTENSION
        })
        .collect::<Vec<std::path::PathBuf>>(),
    ),
  }
}
