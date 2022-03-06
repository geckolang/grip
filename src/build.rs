use crate::package;
use gecko::lint::Lint;
use gecko::llvm_lowering::Lower;
use gecko::name_resolution::Resolve;
use gecko::semantic_check::SemanticCheck;

pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

/// Serves as the driver for the Gecko compiler.
///
/// Can be used to compile a single file, or multiple, and produce
/// a single LLVM module.
pub struct Driver<'a, 'ctx> {
  pub source_files: Vec<std::path::PathBuf>,
  pub file_contents: std::collections::HashMap<std::path::PathBuf, String>,
  pub llvm_module: &'a inkwell::module::Module<'ctx>,
  cache: gecko::cache::Cache,
  name_resolver: gecko::name_resolution::NameResolver,
  lint_context: gecko::lint::LintContext,
  type_context: gecko::semantic_check::SemanticCheckContext,
  llvm_generator: gecko::llvm_lowering::LlvmGenerator<'a, 'ctx>,
}

impl<'a, 'ctx> Driver<'a, 'ctx> {
  pub fn new(
    llvm_context: &'ctx inkwell::context::Context,
    llvm_module: &'a inkwell::module::Module<'ctx>,
  ) -> Self {
    Self {
      source_files: Vec::new(),
      file_contents: std::collections::HashMap::new(),
      llvm_module,
      cache: gecko::cache::Cache::new(),
      name_resolver: gecko::name_resolution::NameResolver::new(),
      lint_context: gecko::lint::LintContext::new(),
      type_context: gecko::semantic_check::SemanticCheckContext::new(),
      llvm_generator: gecko::llvm_lowering::LlvmGenerator::new(llvm_context, &llvm_module),
    }
  }

  /// Attempt to retrieve a node's unique id.
  ///
  /// If the node is not a top-level node (a definition), `None` will
  /// be returned.
  fn find_unique_id(node: &gecko::ast::Node) -> Option<gecko::cache::UniqueId> {
    Some(match &node.kind {
      gecko::ast::NodeKind::Function(function) => function.unique_id,
      // TODO: Missing cases.
      _ => return None,
    })
  }

  fn read_and_lex(&self, source_file: &std::path::PathBuf) -> Vec<gecko::lexer::Token> {
    // FIXME: Performing unsafe operations temporarily.

    let source_code = package::fetch_file_contents(&source_file).unwrap();
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

  pub fn build(&mut self) -> Vec<gecko::diagnostic::Diagnostic> {
    // FIXME: This function may be too complex (too many loops). Find a way to simplify the loops?

    let mut diagnostics = Vec::new();
    let mut module_map = std::collections::HashMap::new();

    // Read, lex, parse, perform name resolution (declarations)
    // and collect the AST (top-level nodes) from each source file.
    for source_file in &self.source_files {
      let tokens = self.read_and_lex(source_file);
      let mut parser = gecko::parser::Parser::new(tokens, &mut self.cache);

      let top_level_nodes = match parser.parse_all() {
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

      // Give ownership of the top-level nodes to the cache.
      for top_level_node in top_level_nodes.into_iter() {
        // TODO: Unsafe unwrap.
        let unique_id = Self::find_unique_id(&top_level_node).unwrap();

        self
          .cache
          .new_symbol_table
          .insert(unique_id, top_level_node);

        module_map.insert(unique_id, source_file_name.clone());
      }

      for top_level_node in self.cache.new_symbol_table.values() {
        top_level_node.declare(&mut self.name_resolver, &self.cache);
      }
    }

    // After all the ASTs have been collected, perform actual name resolution.
    for (unique_id, top_level_node) in &mut self.cache.new_symbol_table {
      self
        .name_resolver
        .set_active_module(module_map.get(unique_id).unwrap().clone());

      top_level_node.resolve(&mut self.name_resolver);
    }

    diagnostics.extend(self.name_resolver.diagnostic_builder.diagnostics.clone());

    // Cannot continue to other phases if name resolution failed.
    if diagnostics.iter().any(|x| x.is_error_like()) {
      return diagnostics;
    }

    // Once symbols are resolved, we can proceed to the other phases.
    for top_level_node in self.cache.new_symbol_table.values() {
      top_level_node.check(&mut self.type_context, &self.cache);

      // TODO: Can we mix linting with type-checking without any problems?
      top_level_node.lint(&self.cache, &mut self.lint_context);
    }

    diagnostics.extend(self.type_context.diagnostic_builder.diagnostics.clone());
    diagnostics.extend(self.lint_context.diagnostic_builder.diagnostics.clone());

    // TODO: Any way for better efficiency (less loops)?
    // Lowering cannot proceed if there was an error.
    if diagnostics.iter().any(|x| x.is_error_like()) {
      return diagnostics;
    }

    // Once symbols are resolved, we can proceed to the other phases.
    for top_level_node in self.cache.new_symbol_table.values() {
      // TODO: Unsafe unwrap.
      let unique_id = Self::find_unique_id(top_level_node).unwrap();

      // TODO: Unsafe access.
      // TODO: In the future, we need to get rid of the `unique_id` property on `Node`s.
      self.llvm_generator.module_name = module_map.get(&unique_id).unwrap().clone();

      top_level_node.lower(&mut self.llvm_generator, &self.cache);
    }

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
    top_level_node.resolve(name_resolver);
  }

  let mut diagnostics: Vec<gecko::diagnostic::Diagnostic> =
    lint_context.diagnostic_builder.diagnostics.clone();

  let mut error_encountered = diagnostics
    .iter()
    .find(|diagnostic| diagnostic.is_error_like())
    .is_some();

  // Cannot continue to any more phases if name resolution failed.
  if !error_encountered {
    let mut semantic_context = gecko::semantic_check::SemanticCheckContext::new();

    // Perform type-checking.
    for top_level_node in &mut top_level_nodes {
      top_level_node.check(&mut semantic_context, &mut cache);
    }

    diagnostics.extend::<Vec<_>>(semantic_context.diagnostic_builder.into());

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
