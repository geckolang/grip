use crate::package;
use gecko::lint::Lint;
use gecko::llvm_lowering::Lower;
use gecko::name_resolution::Resolvable;
use gecko::type_check::TypeCheck;
use std::str::FromStr;

pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

// TODO: Consider returning a `Vec<diagnostic::Diagnostic>` containing the actual problem(s) encountered.
// TODO: Merge `source_file_name` and `source_file_contents` into a tuple.
pub fn build_single_file<'ctx>(
  llvm_context: &'ctx inkwell::context::Context,
  llvm_module: &inkwell::module::Module<'ctx>,
  source_file_name: String,
  source_file_contents: &String,
  build_arg_matches: &clap::ArgMatches<'_>,
) -> Vec<gecko::diagnostic::Diagnostic> {
  let tokens_result = gecko::lexer::Lexer::from_str(source_file_contents).lex_all();

  // TODO: Can't lexing report more than a single diagnostic? Also, it needs to be verified that the reported diagnostics are erroneous.
  if let Err(diagnostic) = tokens_result {
    return vec![diagnostic];
  }

  // Filter tokens to only include those that are relevant (ignore whitespace, comments, etc.).
  let tokens: Vec<gecko::token::Token> = tokens_result
    .unwrap()
    .into_iter()
    .filter(|token| match token.0 {
      gecko::token::TokenKind::Whitespace(_) | gecko::token::TokenKind::Comment(_) => false,
      _ => true,
    })
    .collect();

  if build_arg_matches.is_present(crate::ARG_LIST_TOKENS) {
    // TODO: Better printing.
    println!("tokens: {:?}\n\n", tokens.clone());
  }

  let mut context = gecko::context::Context::new();
  let mut parser = gecko::parser::Parser::new(tokens, &mut context);
  let top_level_nodes_result = parser.parse_all();

  // TODO: Can't parsing report more than a single diagnostic? Also, it needs to be verified that the reported diagnostics are erroneous.
  if let Err(diagnostic) = top_level_nodes_result {
    return vec![diagnostic];
  }

  let mut top_level_nodes = top_level_nodes_result.unwrap();
  let mut diagnostics = Vec::new();
  let mut name_resolver = gecko::name_resolution::NameResolver::new();

  for top_level_node in &mut top_level_nodes {
    top_level_node.declare(&mut name_resolver, &mut context);
  }

  for top_level_node in &mut top_level_nodes {
    top_level_node.resolve(&mut name_resolver, &mut context);
  }

  diagnostics.extend::<Vec<_>>(name_resolver.diagnostics.into());

  let mut error_encountered = diagnostics
    .iter()
    .find(|diagnostic| diagnostic.is_error_like())
    .is_some();

  // Cannot continue to any more phases if name resolution failed.
  if !error_encountered {
    let mut type_context = gecko::type_check::TypeCheckContext::new();

    // Perform type-checking.
    for top_level_node in &mut top_level_nodes {
      top_level_node.type_check(&mut type_context, &mut context);
    }

    diagnostics.extend::<Vec<_>>(type_context.diagnostics.into());

    let mut lint_context = gecko::lint::LintContext::new();

    // Perform linting.
    for top_level_node in &mut top_level_nodes {
      top_level_node.lint(&mut context, &mut lint_context);
    }

    lint_context.finalize(&context);
    diagnostics.extend::<Vec<_>>(lint_context.diagnostics.into());

    error_encountered = diagnostics
      .iter()
      .find(|diagnostic| diagnostic.is_error_like())
      .is_some();

    // Do not attempt to lower if there were any errors.
    if !error_encountered {
      let mut llvm_generator =
        gecko::llvm_lowering::LlvmGenerator::new(source_file_name, llvm_context, &llvm_module);

      for top_level_node in top_level_nodes {
        top_level_node.lower(&mut llvm_generator, &mut context);
      }
    }

    // TODO: Collect lowering diagnostics if any? There are none right now.
  }
  
  diagnostics
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
      .template("building: {msg} [{bar:15}] {pos}/{len} {elapsed_precise}"),
  );

  for path in source_directories {
    // TODO: File names need to conform to identifier rules.
    let source_file_name = path.file_stem().unwrap().to_string_lossy().to_string();

    progress_bar.set_message(format!("{}", source_file_name));

    // TODO: Clear progress bar on error.
    let source_file_contents = package::fetch_source_file_contents(&path)?;

    let build_diagnostics = build_single_file(
      &llvm_context,
      &llvm_module,
      source_file_name,
      &source_file_contents,
      &build_arg_matches,
    );

    if !build_diagnostics.is_empty() {
      let mut error_encountered = false;

      for diagnostic in build_diagnostics {
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

  // TODO: Should the output file's path be handled here?
  let mut output_file_path = std::path::PathBuf::from(package_manifest.name.clone());

  output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);
  assert!(llvm_module.verify().is_ok());

  Ok((llvm_module.print_to_string().to_string(), output_file_path))
}
