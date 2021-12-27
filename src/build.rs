use crate::package;
use gecko::llvm_lowering::Lower;
use gecko::name_resolution::Resolvable;
use std::str::FromStr;

pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

// TODO: Consider returning a `Vec<diagnostic::Diagnostic>` containing the actual problem(s) encountered.
pub fn build_single_file<'ctx>(
  llvm_context: &'ctx inkwell::context::Context,
  llvm_module: &inkwell::module::Module<'ctx>,
  source_file_name: String,
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
      gecko::token::Token::Whitespace(_) | gecko::token::Token::Comment(_) => false,
      _ => true,
    })
    .collect();

  if build_arg_matches.is_present(crate::ARG_LIST_TOKENS) {
    // TODO: Better printing.
    println!("tokens: {:?}\n\n", tokens.clone());
  }

  let mut context = gecko::context::Context::new();
  let mut parser = gecko::parser::Parser::new(tokens, &mut context);

  // TODO: Parse all possible modules.

  let top_level_nodes_result = parser.parse_all();

  if let Err(diagnostic) = top_level_nodes_result {
    return Err(vec![diagnostic]);
  }

  let mut top_level_nodes = top_level_nodes_result.unwrap();
  let diagnostics = Vec::new();

  // FIXME: Perform name resolution.
  // FIXME: Perform type checking.

  // TODO: Better code structure for this flag.
  let encountered_error = false;

  // TODO:
  // for diagnostic in &diagnostics {
  //   if diagnostic.is_error_like() {
  //     encountered_error = true;
  //   }
  // }

  let mut name_resolver = gecko::name_resolution::NameResolver::new();

  // TODO: Any way to simplify from having too loops/passes into one?
  for top_level_node in &mut top_level_nodes {
    top_level_node.declare(&mut name_resolver, &mut context);

    // TODO: Perform type-checking here as well?
  }

  for top_level_node in &mut top_level_nodes {
    top_level_node.resolve(&mut name_resolver, &mut context);
  }

  // Do not lower if there are errors.
  if !encountered_error {
    let mut llvm_generator =
      gecko::llvm_lowering::LlvmGenerator::new(source_file_name, llvm_context, &llvm_module);

    for top_level_node in top_level_nodes {
      top_level_node.lower(&mut llvm_generator, &mut context);
    }

    // TODO: Collect lowering diagnostics if any? There is none right now.
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
) -> Result<(String, std::path::PathBuf), String> {
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
    // TODO: File names need to conform to identifier rules.
    let source_file_name = path.file_stem().unwrap().to_string_lossy().to_string();

    progress_bar.set_message(source_file_name.clone());

    // TODO: Clear progress bar on error.
    let source_file_contents = package::fetch_source_file_contents(&path)?;

    if let Err(diagnostics) = build_single_file(
      &llvm_context,
      &llvm_module,
      source_file_name,
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

  progress_bar.finish();

  // TODO: In the future, use the appropriate time unit (min, sec, etc.) instead of just `s`.
  log::info!(
    "built package `{}` in {}s",
    package_manifest.name,
    progress_bar.elapsed().as_secs()
  );

  // TODO: Should the output file's path be handled here?
  let mut output_file_path = std::path::PathBuf::from(package_manifest.name.clone());

  output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);

  Ok((llvm_module.print_to_string().to_string(), output_file_path))
}
