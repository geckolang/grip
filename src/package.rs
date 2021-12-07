pub const PATH_MANIFEST_FILE: &str = "grip.toml";
const PATH_SOURCE_FILE_EXTENSION: &str = "ko";
pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

#[derive(serde::Serialize, serde::Deserialize)]
pub struct PackageManifest {
  pub name: String,
  pub version: String,
}

// TODO: Make use of return value.
// TODO: Pass in sub-command matches instead.
pub fn init_package_manifest(matches: &clap::ArgMatches<'_>) -> bool {
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

  if std::fs::create_dir(crate::DEFAULT_SOURCES_DIR).is_err() {
    log::error!("failed to create sources directory");

    return false;
  } else if std::fs::create_dir(crate::DEFAULT_OUTPUT_DIR).is_err() {
    log::error!("failed to create output directory");

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
    version: String::from("0.0.1"),
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

// TODO: Consider returning a `Vec<diagnostic::Diagnostic>` containing the actual problem(s) encountered.
pub fn build_single_file<'ctx>(
  llvm_context: &'ctx inkwell::context::Context,
  llvm_module: &inkwell::module::Module<'ctx>,
  source_file_contents: &String,
  matches: &clap::ArgMatches<'_>,
) -> Result<(), Vec<gecko::diagnostic::Diagnostic>> {
  let tokens_result =
    gecko::lexer::Lexer::new(source_file_contents.chars().collect()).collect_tokens();

  if let Err(diagnostic) = tokens_result {
    return Err(vec![diagnostic]);
  }

  // Filter tokens to only include those that are relevant (ignore whitespace, comments, etc.).
  let tokens: Vec<gecko::token::Token> = tokens_result
    .unwrap()
    .into_iter()
    .filter(|token| match token {
      gecko::token::Token::Whitespace(_) => false,
      gecko::token::Token::Comment(_) => false,
      _ => true,
    })
    .collect();

  if matches.is_present(crate::ARG_LIST_TOKENS) {
    // TODO: Better printing.
    println!("tokens: {:?}\n\n", tokens.clone());
  }

  let mut parser = gecko::parser::Parser::new(tokens);

  // TODO: Parse all possible modules.

  let module_result = parser.parse_module();

  if let Err(diagnostic) = module_result {
    return Err(vec![diagnostic]);
  }

  let mut module = module_result.unwrap();
  let mut diagnostics = Vec::new();

  // FIXME: Perform name resolution.

  if let Some(type_check_diagnostics) = gecko::type_check::type_check_module(&mut module) {
    diagnostics.extend(type_check_diagnostics);
  }

  // TODO: Better code structure for this flag.
  let mut encountered_error = false;

  for diagnostic in &diagnostics {
    if diagnostic.is_error_like() {
      encountered_error = true;
    }
  }

  // Do not lower if there are errors.
  if !encountered_error {
    let mut llvm_lowering = gecko::llvm_lowering::LlvmLowering::new(&llvm_context, llvm_module);

    if let Err(diagnostic) = llvm_lowering.lower_module(&module) {
      diagnostics.push(diagnostic);
    }
  }

  // TODO: Diagnostics vector may only contain non-error diagnostics. What if that's the case?
  return if diagnostics.is_empty() {
    Ok(())
  } else {
    Err(diagnostics)
  };
}

pub fn build_package<'a>(
  llvm_context: &'a inkwell::context::Context,
  matches: &clap::ArgMatches<'_>,
) -> Option<(inkwell::module::Module<'a>, std::path::PathBuf)> {
  let manifest_file_contents = std::fs::read_to_string(PATH_MANIFEST_FILE);

  if manifest_file_contents.is_err() {
    log::error!("path to package manifest does not exist or is inaccessible; run `grip --init` to initialize a default one in the current directory");

    return None;
  }

  let manifest_toml_result =
    toml::from_str::<PackageManifest>(manifest_file_contents.unwrap().as_str());

  if let Err(error) = manifest_toml_result {
    log::error!("failed to parse manifest file: {}", error);

    return None;
  }

  let manifest_toml = manifest_toml_result.unwrap();

  let source_directory_paths_result = std::fs::read_dir(
    matches
      .subcommand_matches(crate::ARG_BUILD)
      .unwrap()
      .value_of(crate::ARG_BUILD_SOURCES_DIR)
      .unwrap(),
  );

  if let Err(error) = source_directory_paths_result {
    log::error!("failed to read sources directory: {}", error);

    return None;
  }

  let source_directory_paths = source_directory_paths_result
    .unwrap()
    .map(|path_result| path_result.unwrap().path())
    .filter(|path| {
      if !path.is_file() {
        return false;
      }

      let extension = path.extension();

      extension.is_some() && extension.unwrap() == PATH_SOURCE_FILE_EXTENSION
    })
    .collect::<Vec<_>>();

  let llvm_module =
      // TODO: Prefer usage of `.file_prefix()` once it is stable.
      llvm_context.create_module(manifest_toml.name.as_str());

  let progress_bar = indicatif::ProgressBar::new(source_directory_paths.len() as u64);

  progress_bar.set_style(
    indicatif::ProgressStyle::default_bar()
      .template("building: {msg} [{bar:30}] {pos}/{len} {elapsed_precise}"),
  );

  for path in source_directory_paths {
    let source_file_name = path.file_name().unwrap().to_string_lossy();

    progress_bar.set_message(format!("{}", source_file_name));

    let source_file_contents_result = fetch_source_file_contents(&path);

    if let Err(error) = source_file_contents_result {
      progress_bar.finish_and_clear();
      log::error!("failed to fetch source file contents: {}", error);

      return None;
    }

    let source_file_contents = source_file_contents_result.unwrap();

    if let Err(diagnostics) =
      build_single_file(&llvm_context, &llvm_module, &source_file_contents, &matches)
    {
      let mut error_encountered = false;

      for diagnostic in diagnostics {
        // TODO: Maybe fix this by clearing then re-writing the progress bar.
        // FIXME: This will interfere with the progress bar (leave it behind).
        crate::console::print_diagnostic(
          vec![(&path.to_str().unwrap().to_string(), &source_file_contents)],
          &diagnostic,
        );

        if diagnostic.is_error_like() {
          error_encountered = true;
        }
      }

      if error_encountered {
        // TODO: Maybe fix this by clearing then re-writing the progress bar.
        // FIXME: This will interfere with the progress bar (leave it behind).
        log::error!(
          "failed to build package `{}` due to previous error(s)",
          manifest_toml.name
        );

        return None;
      }
    }

    progress_bar.inc(1);
  }

  progress_bar.finish_and_clear();

  // TODO: In the future, use the appropriate time unit (min, sec, etc.) instead of just `s`.
  log::info!(
    "built package `{}` in {}s",
    manifest_toml.name,
    progress_bar.elapsed().as_secs()
  );

  let mut output_file_path = std::path::PathBuf::from(manifest_toml.name.clone());

  output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);

  Some((llvm_module, output_file_path))
}
