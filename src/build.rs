use crate::{package, pass};
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

    let mut ast_map = std::collections::BTreeMap::new();

    // Read, lex, parse, perform name resolution (declarations)
    // and collect the AST (top-level nodes) from each source file.
    for (package_name, source_file) in &self.source_files {
      let tokens = self.read_and_lex(source_file);
      let mut parser = gecko::parser::Parser::new(tokens, &mut self.cache);

      let root_nodes = match parser.parse_all() {
        Ok(nodes) => nodes,
        Err(diagnostic) => return vec![diagnostic],
      }
      .into_iter()
      .map(|root_node| std::rc::Rc::new(root_node))
      .collect::<Vec<_>>();

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

    ////////////////////////////////////////////////////////////////////////////

    // TODO: Unsafe unwrap.
    let root_node = ast_map.values().flatten().find(|node| {
          matches!(&node.kind, gecko::ast::NodeKind::Function(function) if function.name == gecko::lowering::MAIN_FUNCTION_NAME)
        }).unwrap().to_owned();

    // TODO:
    // codespan_reporting::diagnostic::Diagnostic::error().with_message("no main function defined")

    let module_qualifier: gecko::name_resolution::Qualifier = gecko::name_resolution::Qualifier {
      module_name: "test_mod".to_string(),
      package_name: "test_pkg".to_string(),
    };

    let mut pass_manager = pass::PassManager::new();

    pass_manager.add_name_resolution_decl(module_qualifier.clone(), std::rc::Rc::clone(&root_node));
    pass_manager.add_name_resolution_link(module_qualifier.clone(), std::rc::Rc::clone(&root_node));
    pass_manager.add_type_inference(root_node.clone());
    pass_manager.add_analysis(root_node.clone());

    // FIXME: This should only be reported if the package is a binary/executable?
    pass_manager.add_lowering("pending", root_node.clone());

    pass_manager.run()

    // TODO: We should have diagnostics ordered/sorted (by severity then phase).
    //pass_manager.name_resolution_decl(module_qualifier.clone(), std::rc::Rc::clone(&root_node));
    // .then(Box::new(|| pass_manager.type_inference(root_node.clone())))
    // .then(Box::new(|| pass_manager.analysis(root_node.clone())))
    // .then(Box::new(|| {
    //   // FIXME: This should only be reported if the package is a binary/executable?
    //   pass_manager.lowering("pending", root_node.clone())
    // }))
    // .run();

    // d
    // vec![]
    ////////////////////////////////////////////////////////////////////////////

    // BUG: Extern functions shouldn't be lowered directly. They are no longer under a wrapper
    // ... node, which ensures their caching. This means that, first they will be forcefully lowered
    // ... here (without caching), then when referenced, since they haven't been cached.
    // Once symbols are resolved, we can proceed to the other phases.
    // for root_node in &readonly_ast {
    //   if let gecko::ast::NodeKind::Function(function) = &root_node.kind {
    //     // Only lower the main function.
    //     if function.name == gecko::lowering::MAIN_FUNCTION_NAME {
    //       // TODO:
    //       // root_node.lower(&mut self.llvm_generator, &self.cache, false);

    //       // TODO: Need to manually cache the main function here. This is because
    //       // ... if it is called once again, since it isn't cached, it will be re-lowered.
    //     }
    //   }
    // }
    // diagnostics
  }
}
