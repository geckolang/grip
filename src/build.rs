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
  pub source_files: Vec<(String, std::path::PathBuf)>,
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

  // REVIEW: Consider accepting the source files here? More strict?
  pub fn build(&mut self) -> Vec<gecko::diagnostic::Diagnostic> {
    // FIXME: This function may be too complex (too many loops). Find a way to simplify the loops?

    let mut diagnostics = Vec::new();

    // REVISE: Just use `module_map`, but `String -> Vec<Node>` or `Node -> String`.
    let mut module_map = std::collections::HashMap::new();
    let mut ast = std::collections::HashMap::new();

    // Read, lex, parse, perform name resolution (declarations)
    // and collect the AST (top-level nodes) from each source file.
    for (package_name, source_file) in &self.source_files {
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

      let global_qualifier = (package_name.clone(), source_file_name.clone());

      // FIXME: Not only top-level nodes should be registered on the cache. What about parameters?
      // Give ownership of the top-level nodes to the cache.
      for root_node in &root_nodes {
        // REVISE: Unsafe unwrap.
        let unique_id = Self::find_unique_id(&root_node).unwrap();

        module_map.insert(unique_id, global_qualifier.clone());
      }

      ast.insert(global_qualifier.clone(), root_nodes);
    }

    // After all the ASTs have been collected, perform name resolution.
    diagnostics.extend(self.name_resolver.run(&mut ast, &mut self.cache));

    if self.cache.main_function_id.is_none() {
      diagnostics.push(gecko::diagnostic::Diagnostic {
        severity: gecko::diagnostic::Severity::Error,
        message: "no main function defined".to_string(),
        span: None,
      });
    }

    // Cannot continue to other phases if name resolution failed.
    if diagnostics
      .iter()
      .any(|diagnostic| diagnostic.severity == gecko::diagnostic::Severity::Error)
    {
      return diagnostics;
    }

    let readonly_ast = ast
      .into_values()
      .flatten()
      .into_iter()
      .map(|node| std::rc::Rc::new(node))
      .collect::<Vec<_>>();

    // Once symbols are resolved, we can proceed to the other phases.
    for root_node in &readonly_ast {
      root_node.check(&mut self.type_context, &self.cache);

      // TODO: Can we mix linting with type-checking without any problems?
      root_node.lint(&self.cache, &mut self.lint_context);
    }

    self.lint_context.finalize(&self.cache);

    let semantic_check_result =
      gecko::semantic_check::SemanticCheckContext::run(&readonly_ast, &self.cache);

    // FIXME: Make use of the returned imports!

    diagnostics.extend(semantic_check_result.0);
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
          self.llvm_generator.module_name = module_map.get(&unique_id).unwrap().1.clone();

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
