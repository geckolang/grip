use crate::package;
use std::str::FromStr;

pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

// TODO: Consider returning a `Vec<diagnostic::Diagnostic>` containing the actual problem(s) encountered.
pub fn build_single_file<'ctx>(
  llvm_context: &'ctx inkwell::context::Context,
  llvm_module: &inkwell::module::Module<'ctx>,
  source_file_contents: &String,
  build_arg_matches: &clap::ArgMatches<'_>,
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

  if build_arg_matches.is_present(crate::ARG_LIST_TOKENS) {
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
  build_arg_matches: &clap::ArgMatches<'_>,
) -> Result<(inkwell::module::Module<'a>, std::path::PathBuf), String> {
  let package_manifest = package::read_manifest()?;

  let source_directories =
    package::read_sources_dir(&std::path::PathBuf::from_str(crate::DEFAULT_SOURCES_DIR).unwrap())?;

  let llvm_module =
      // TODO: Prefer usage of `.file_prefix()` once it is stable.
      llvm_context.create_module(package_manifest.name.as_str());

  let progress_bar = indicatif::ProgressBar::new(source_directories.len() as u64);

  progress_bar.set_style(
    indicatif::ProgressStyle::default_bar()
      .template("building: {msg} [{bar:30}] {pos}/{len} {elapsed_precise}"),
  );

  for path in source_directories {
    let source_file_name = path.file_name().unwrap().to_string_lossy();

    progress_bar.set_message(format!("{}", source_file_name));

    // TODO: Clear progress bar on error.
    let source_file_contents = package::fetch_source_file_contents(&path)?;

    if let Err(diagnostics) = build_single_file(
      &llvm_context,
      &llvm_module,
      &source_file_contents,
      &build_arg_matches,
    ) {
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
        return Err(format!(
          "failed to build package `{}` due to previous error(s)",
          package_manifest.name
        ));
      }
    }

    progress_bar.inc(1);
  }

  progress_bar.finish_and_clear();

  // TODO: In the future, use the appropriate time unit (min, sec, etc.) instead of just `s`.
  log::info!(
    "built package `{}` in {}s",
    package_manifest.name,
    progress_bar.elapsed().as_secs()
  );

  let mut output_file_path = std::path::PathBuf::from(package_manifest.name.clone());

  output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);

  Ok((llvm_module, output_file_path))
}
