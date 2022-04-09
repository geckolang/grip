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
  fn find_unique_id(node: &gecko::ast::Node) -> Option<gecko::cache::BindingId> {
    Some(match &node.kind {
      gecko::ast::NodeKind::Function(function) => function.binding_id,
      gecko::ast::NodeKind::Enum(enum_) => enum_.binding_id,
      gecko::ast::NodeKind::StructType(struct_type) => struct_type.binding_id,
      gecko::ast::NodeKind::TypeAlias(type_alias) => type_alias.binding_id,
      gecko::ast::NodeKind::ExternFunction(extern_function) => extern_function.binding_id,
      gecko::ast::NodeKind::ExternStatic(extern_static) => extern_static.binding_id,
      // REVIEW: Missing cases?
      _ => return None,
    })
  }

  fn read_and_lex(&self, source_file: &std::path::PathBuf) -> Vec<gecko::lexer::Token> {
    // FIXME: Performing unsafe operations temporarily.

    let source_code = package::fetch_file_contents(&source_file).unwrap();
    let tokens = gecko::lexer::Lexer::from_str(source_code.as_str()).lex_all();

    // BUG: This will fail if there were lexing errors. Unsafe unwrap.
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

    // REVISE: Just use `module_map`, but `String -> Vec<Node>` or `Node -> String`.
    let mut module_map = std::collections::HashMap::new();
    let mut ast = Vec::new();

    // Read, lex, parse, perform name resolution (declarations)
    // and collect the AST (top-level nodes) from each source file.
    for source_file in &self.source_files {
      let tokens = self.read_and_lex(source_file);
      let mut parser = gecko::parser::Parser::new(tokens, &mut self.cache);

      let root_nodes = match parser.parse_all() {
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

      // FIXME: Not only top-level nodes should be registered on the cache. What about parameters?
      // Give ownership of the top-level nodes to the cache.
      for root_node in &root_nodes {
        root_node.declare(&mut self.name_resolver);

        // REVISE: Unsafe unwrap.
        let unique_id = Self::find_unique_id(&root_node).unwrap();

        module_map.insert(unique_id, source_file_name.clone());
      }

      ast.extend(root_nodes);
    }

    // After all the ASTs have been collected, perform name resolution.
    for root_node in &mut ast {
      // TODO:
      // self
      //   .name_resolver
      //   .set_active_module(module_map.get(unique_id).unwrap().clone());

      root_node.resolve(&mut self.name_resolver, &mut self.cache);
    }

    diagnostics.extend(self.name_resolver.diagnostic_builder.diagnostics.clone());

    // Cannot continue to other phases if name resolution failed.
    if diagnostics
      .iter()
      .any(|diagnostic| diagnostic.severity == gecko::diagnostic::Severity::Error)
    {
      return diagnostics;
    }

    let readonly_ast = ast
      .into_iter()
      .map(|node| std::rc::Rc::new(node))
      .collect::<Vec<_>>();

    // Once symbols are resolved, we can proceed to the other phases.
    for root_node in &readonly_ast {
      root_node.check(&mut self.type_context, &self.cache);

      // TODO: Can we mix linting with type-checking without any problems?
      root_node.lint(&self.cache, &mut self.lint_context);
    }

    diagnostics.extend(self.type_context.diagnostic_builder.diagnostics.clone());
    diagnostics.extend(self.lint_context.diagnostic_builder.diagnostics.clone());

    // TODO: Any way for better efficiency (less loops)?
    // Lowering cannot proceed if there was an error.
    if diagnostics
      .iter()
      .any(|diagnostic| diagnostic.severity == gecko::diagnostic::Severity::Error)
    {
      return diagnostics;
    }

    // REVISE: For efficiency, and to solve caching issues, only lower the `main` function here.
    // ... Any referenced entity within it (thus the whole program) will be lowered and cached
    // ... accordingly from there on.
    // BUG: Extern functions shouldn't be lowered directly. They are no longer under a wrapper
    // ... node, which ensures their caching. This means that, first they will be forcefully lowered
    // ... here (without caching), then when referenced, since they haven't been cached.
    // Once symbols are resolved, we can proceed to the other phases.
    for root_node in &readonly_ast {
      if let gecko::ast::NodeKind::Function(function) = &root_node.kind {
        // Only lower the main function.
        if function.name == gecko::llvm_lowering::MAIN_FUNCTION_NAME {
          // TODO: Unsafe unwrap.
          let unique_id = Self::find_unique_id(&root_node).unwrap();

          // TODO: Unsafe access.
          // TODO: In the future, we need to get rid of the `unique_id` property on `Node`s.
          self.llvm_generator.module_name = module_map.get(&unique_id).unwrap().clone();

          root_node.lower(&mut self.llvm_generator, &self.cache);

          // TODO: Need to manually cache the main function here. This is because
          // ... if it is called once again, since it isn't cached, it will be re-lowered.
        }
      }
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
  let root_nodes_result = parser.parse_all();

  // TODO: Can't parsing report more than a single diagnostic? Also, it needs to be verified that the reported diagnostics are erroneous.
  if let Err(diagnostic) = root_nodes_result {
    return vec![diagnostic];
  }

  let mut root_nodes = root_nodes_result.unwrap();

  for root_node in &mut root_nodes {
    root_node.declare(name_resolver);
  }

  for root_node in &mut root_nodes {
    root_node.resolve(name_resolver, &mut cache);
  }

  let mut diagnostics: Vec<gecko::diagnostic::Diagnostic> =
    lint_context.diagnostic_builder.diagnostics.clone();

  let mut error_encountered = diagnostics
    .iter()
    .any(|diagnostic| diagnostic.severity == gecko::diagnostic::Severity::Error);

  // Cannot continue to any more phases if name resolution failed.
  if !error_encountered {
    let mut semantic_context = gecko::semantic_check::SemanticCheckContext::new();

    // Perform type-checking.
    for root_node in &mut root_nodes {
      root_node.check(&mut semantic_context, &mut cache);
    }

    diagnostics.extend::<Vec<_>>(semantic_context.diagnostic_builder.into());

    // Perform linting.
    for root_node in &mut root_nodes {
      root_node.lint(&mut cache, lint_context);
    }

    // TODO: Ensure this doesn't affect multiple files.
    lint_context.finalize(&cache);
    diagnostics.extend::<Vec<_>>(lint_context.diagnostic_builder.diagnostics.clone());

    error_encountered = diagnostics
      .iter()
      .any(|diagnostic| diagnostic.severity == gecko::diagnostic::Severity::Error);

    // Do not attempt to lower if there were any errors.
    if !error_encountered {
      for root_node in root_nodes {
        root_node.lower(llvm_generator, &mut cache);
      }
    }

    // TODO: Collect lowering diagnostics if any? There are none right now.
  }

  diagnostics
}
