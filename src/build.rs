use crate::package;
use gecko::lint::Lint;
use gecko::llvm_lowering::Lower;
use gecko::name_resolution::Resolve;
use gecko::type_check::TypeCheck;
use std::str::FromStr;

pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

/// Serves as the driver for the Gecko compiler.
///
/// Can be used to compile a single file, or multiple, and produce
/// a single LLVM module.
pub struct ProjectBuilder<'a, 'ctx> {
  pub source_files: Vec<std::path::PathBuf>,
  pub llvm_module: &'a inkwell::module::Module<'ctx>,
  cache: gecko::cache::Cache,
  name_resolver: gecko::name_resolution::NameResolver,
  lint_context: gecko::lint::LintContext,
  type_context: gecko::type_check::TypeCheckContext,
  llvm_generator: gecko::llvm_lowering::LlvmGenerator<'a, 'ctx>,
}

impl<'a, 'ctx> ProjectBuilder<'a, 'ctx> {
  pub fn new(
    llvm_context: &'ctx inkwell::context::Context,
    llvm_module: &'a inkwell::module::Module<'ctx>,
  ) -> Self {
    Self {
      source_files: Vec::new(),
      llvm_module,
      cache: gecko::cache::Cache::new(),
      name_resolver: gecko::name_resolution::NameResolver::new(),
      lint_context: gecko::lint::LintContext::new(),
      type_context: gecko::type_check::TypeCheckContext::new(),
      llvm_generator: gecko::llvm_lowering::LlvmGenerator::new(llvm_context, &llvm_module),
    }
  }

  fn read_and_lex(&self, source_file: &std::path::PathBuf) -> Vec<gecko::lexer::Token> {
    // FIXME: Performing unsafe operations temporarily.

    let source_code = package::fetch_source_file_contents(&source_file).unwrap();
    let tokens = gecko::lexer::Lexer::from_str(source_code.as_str()).lex_all();

    // FIXME: What about illegal tokens?
    // TODO: This might be inefficient for larger programs, so consider passing an option to the lexer.
    // Filter tokens to only include those that are relevant (ignore whitespace, comments, etc.).
    tokens
      .unwrap()
      .into_iter()
      .filter(|token| {
        !matches!(
          token.0,
          gecko::lexer::TokenKind::Whitespace(_) | gecko::lexer::TokenKind::Comment(_)
        )
      })
      .collect()
  }

  pub fn compile(&mut self) -> Vec<gecko::diagnostic::Diagnostic> {
    // FIXME: May be too complex (too many loops). Find a way to simplify the loops?

    let mut ast = std::collections::HashMap::new();
    let mut diagnostics = Vec::new();

    // Read, lex, parse, perform name resolution (declarations)
    // and collect the AST (top-level nodes) from each source file.
    for source_file in &self.source_files {
      let tokens = self.read_and_lex(source_file);
      let mut parser = gecko::parser::Parser::new(tokens, &mut self.cache);

      let mut top_level_nodes = match parser.parse_all() {
        Ok(nodes) => nodes,
        Err(diagnostic) => return vec![diagnostic],
      };

      // TODO: File names need to conform to identifier rules.
      let source_file_name = source_file
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();

      self.name_resolver.create_module(source_file_name.clone());

      for top_level_node in &mut top_level_nodes {
        top_level_node.declare(&mut self.name_resolver, &mut self.cache);
      }

      ast.insert(source_file_name, top_level_nodes);
    }

    // After all the ASTs have been collected, perform actual name resolution.
    for (module_name, inner_ast) in &mut ast {
      self.name_resolver.set_active_module(module_name.clone());

      for top_level_node in inner_ast {
        top_level_node.resolve(&mut self.name_resolver, &mut self.cache);
      }
    }

    diagnostics.extend(self.name_resolver.diagnostic_builder.diagnostics.clone());

    // Cannot continue to other phases if name resolution failed.
    if diagnostics.iter().find(|x| x.is_error_like()).is_some() {
      return diagnostics;
    }

    // Once symbols are resolved, we can proceed to the other phases.
    for inner_ast in ast.values_mut() {
      for top_level_node in inner_ast {
        top_level_node.type_check(&mut self.type_context, &mut self.cache);

        // TODO: Can we mix linting with type-checking without any problems?
        top_level_node.lint(&mut self.cache, &mut self.lint_context);
      }
    }

    // TODO: Any way for better efficiency (less loops)?
    // Lowering cannot proceed if there was an error.
    if diagnostics.iter().find(|x| x.is_error_like()).is_some() {
      return diagnostics;
    }

    // Once symbols are resolved, we can proceed to the other phases.
    for (module_name, inner_ast) in &mut ast {
      self.llvm_generator.module_name = module_name.clone();

      for top_level_node in inner_ast {
        top_level_node.lower(&mut self.llvm_generator, &mut self.cache);
      }
    }

    diagnostics.extend(self.type_context.diagnostic_builder.diagnostics.clone());
    diagnostics.extend(self.lint_context.diagnostic_builder.diagnostics.clone());

    // TODO: We should have diagnostics ordered/sorted (by severity then phase).
    diagnostics
  }
}

pub fn build_single_file<'ctx>(
  source_file: (String, &String),
  build_arg_matches: &clap::ArgMatches<'_>,
  name_resolver: &mut gecko::name_resolution::NameResolver,
  lint_context: &mut gecko::lint::LintContext,
  llvm_generator: &mut gecko::llvm_lowering::LlvmGenerator<'_, 'ctx>,
) -> Vec<gecko::diagnostic::Diagnostic> {
  let tokens_result = gecko::lexer::Lexer::from_str(source_file.1).lex_all();

  // TODO: Can't lexing report more than a single diagnostic? Also, it needs to be verified that the reported diagnostics are erroneous.
  if let Err(diagnostic) = tokens_result {
    return vec![diagnostic];
  }

  // Filter tokens to only include those that are relevant (ignore whitespace, comments, etc.).
  let tokens: Vec<gecko::lexer::Token> = tokens_result
    .unwrap()
    .into_iter()
    .filter(|token| {
      !matches!(
        token.0,
        gecko::lexer::TokenKind::Whitespace(_) | gecko::lexer::TokenKind::Comment(_)
      )
    })
    .collect();

  if build_arg_matches.is_present(crate::ARG_LIST_TOKENS) {
    // TODO: Better printing.
    println!("tokens: {:?}\n\n", tokens.clone());
  }

  let mut cache = gecko::cache::Cache::new();
  let mut parser = gecko::parser::Parser::new(tokens, &mut cache);
  let top_level_nodes_result = parser.parse_all();

  // TODO: Can't parsing report more than a single diagnostic? Also, it needs to be verified that the reported diagnostics are erroneous.
  if let Err(diagnostic) = top_level_nodes_result {
    return vec![diagnostic];
  }

  let mut top_level_nodes = top_level_nodes_result.unwrap();

  for top_level_node in &mut top_level_nodes {
    top_level_node.declare(name_resolver, &mut cache);
  }

  for top_level_node in &mut top_level_nodes {
    top_level_node.resolve(name_resolver, &mut cache);
  }

  let mut diagnostics: Vec<gecko::diagnostic::Diagnostic> =
    lint_context.diagnostic_builder.diagnostics.clone();

  let mut error_encountered = diagnostics
    .iter()
    .find(|diagnostic| diagnostic.is_error_like())
    .is_some();

  // Cannot continue to any more phases if name resolution failed.
  if !error_encountered {
    let mut type_context = gecko::type_check::TypeCheckContext::new();

    // Perform type-checking.
    for top_level_node in &mut top_level_nodes {
      top_level_node.type_check(&mut type_context, &mut cache);
    }

    diagnostics.extend::<Vec<_>>(type_context.diagnostic_builder.into());

    // Perform linting.
    for top_level_node in &mut top_level_nodes {
      top_level_node.lint(&mut cache, lint_context);
    }

    // TODO: Ensure this doesn't affect multiple files.
    lint_context.finalize(&cache);
    diagnostics.extend::<Vec<_>>(lint_context.diagnostic_builder.diagnostics.clone());

    error_encountered = diagnostics
      .iter()
      .find(|diagnostic| diagnostic.is_error_like())
      .is_some();

    // Do not attempt to lower if there were any errors.
    if !error_encountered {
      for top_level_node in top_level_nodes {
        top_level_node.lower(llvm_generator, &mut cache);
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

  let mut name_resolver = gecko::name_resolution::NameResolver::new();
  let mut lint_context = gecko::lint::LintContext::new();

  let mut llvm_generator = gecko::llvm_lowering::LlvmGenerator::new(llvm_context, &llvm_module);

  // FIXME: First we must run name resolution for all the files, then proceed to the other phases.

  // for source_file_path in source_directories {
  //   for top_level_node in &mut top_level_nodes {
  //     top_level_node.declare(name_resolver, &mut cache);
  //   }
  // }

  for source_file_path in source_directories {
    // TODO: File names need to conform to identifier rules.
    let source_file_name = source_file_path
      .file_stem()
      .unwrap()
      .to_string_lossy()
      .to_string();

    progress_bar.set_message(format!("{}", source_file_name));

    // TODO: Clear progress bar on error.
    let source_file_contents = package::fetch_source_file_contents(&source_file_path)?;

    let build_diagnostics = build_single_file(
      (source_file_name, &source_file_contents),
      &build_arg_matches,
      &mut name_resolver,
      &mut lint_context,
      &mut llvm_generator,
    );

    if !build_diagnostics.is_empty() {
      let mut error_encountered = false;

      for diagnostic in build_diagnostics {
        // TODO: Maybe fix this by clearing then re-writing the progress bar.
        // FIXME: This will interfere with the progress bar (leave it behind).
        crate::console::print_diagnostic(
          vec![(
            &source_file_path.to_str().unwrap().to_string(),
            &source_file_contents,
          )],
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

  // Verify that the produced LLVM IR is well-formed (including all functions).
  assert!(llvm_module.verify().is_ok());

  Ok((llvm_module.print_to_string().to_string(), output_file_path))
}
