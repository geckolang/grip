use crate::package;
use gecko::type_system::Check;

/// Serves as the driver for the Gecko compiler.
///
/// Can be used to compile a single file, or multiple, and produce
/// a single LLVM module.
pub struct Driver<'a, 'ctx> {
  pub source_files: Vec<(String, std::path::PathBuf)>,
  pub file_contents: std::collections::HashMap<std::path::PathBuf, String>,
  pub llvm_module: &'a inkwell::module::Module<'ctx>,
  cache: gecko::cache::Cache,
  // name_resolver: gecko::name_resolution::NameResolver,
  lint_context: gecko::lint::LintContext,
  type_context: gecko::type_system::TypeContext,
  // llvm_generator: gecko::lowering::LlvmGenerator<'a, 'ctx>,
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
      // FIXME: Pass the actual expected parameter, instead of this dummy value.
      // name_resolver: gecko::name_resolution::NameResolver::new(gecko::name_resolution::Qualifier {
      //   package_name: String::from("pending_package_name"),
      //   module_name: String::from("pending_module_name"),
      // }),
      lint_context: gecko::lint::LintContext::new(),
      type_context: gecko::type_system::TypeContext::new(),
      // llvm_generator: gecko::lowering::LlvmGenerator::new(llvm_context, &llvm_module),
    }
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
          gecko::lexer::TokenKind::Comment(_) | gecko::lexer::TokenKind::Whitespace(_)
        )
      })
      .collect()
  }

  // REVIEW: Consider accepting the source files here? More strict?
  pub fn build(&mut self) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    // FIXME: Must name the LLVM module with the initial package's name.
    // self.llvm_generator.module_name = "my_project".to_string();

    // FIXME: This function may be too complex (too many loops). Find a way to simplify the loops?

    let mut diagnostics = Vec::new();
    let mut ast_map = std::collections::BTreeMap::new();

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

      ast_map.insert(
        gecko::name_resolution::Qualifier {
          package_name: package_name.clone(),
          module_name: source_file_name.clone(),
        },
        root_nodes,
      );
    }

    // After all the ASTs have been collected, perform name resolution.
    // diagnostics.extend(self.name_resolver.run(&mut ast_map, &mut self.cache));

    // FIXME: This should only be reported if the package is a binary/executable?
    if self.cache.main_function_id.is_none() {
      diagnostics.push(
        codespan_reporting::diagnostic::Diagnostic::error()
          .with_message("no main function defined"),
      );
    }

    // Cannot continue to other phases if name resolution failed.
    if diagnostics
      .iter()
      .any(|diagnostic| diagnostic.severity == codespan_reporting::diagnostic::Severity::Error)
    {
      return diagnostics;
    }

    // Perform global type inference & expansion of type variables.
    for inner_ast in ast_map.values_mut() {
      for root_node in inner_ast.iter_mut() {
        // TODO:
        // root_node
        //   .kind
        //   .post_unification(&mut self.type_context, &self.cache);
      }
    }

    let readonly_ast = ast_map
      .into_values()
      .flatten()
      .into_iter()
      // REVIEW: Why did we have it be `Rc<>` in the first place?
      // .map(|node| std::rc::Rc::new(node))
      .collect::<Vec<_>>();

    // Once symbols are resolved, we can proceed to the other phases.
    for root_node in &readonly_ast {
      // root_node.kind.traverse(|node| {
      //   node.check(&mut self.type_context, &self.cache);
      //   node.lint(&mut self.lint_context);

      //   true
      // });
    }

    self.lint_context.finalize(&self.cache);

    let type_check_result = gecko::type_system::TypeContext::run(&readonly_ast, &self.cache);

    // FIXME: Make use of the returned imports!

    diagnostics.extend(type_check_result);
    diagnostics.extend(self.lint_context.diagnostics.clone());

    // TODO: Any way for better efficiency (less loops)?
    // Lowering cannot proceed if there was an error.
    if diagnostics
      .iter()
      .any(|diagnostic| diagnostic.severity == codespan_reporting::diagnostic::Severity::Error)
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
        if function.name == gecko::lowering::MAIN_FUNCTION_NAME {
          // TODO:
          // root_node.lower(&mut self.llvm_generator, &self.cache, false);

          // TODO: Need to manually cache the main function here. This is because
          // ... if it is called once again, since it isn't cached, it will be re-lowered.
        }
      }
    }

    // TODO: We should have diagnostics ordered/sorted (by severity then phase).
    diagnostics
  }
}
