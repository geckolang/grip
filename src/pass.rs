use gecko::visitor::AnalysisVisitor;

pub type PassAction = dyn FnOnce() -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>>;

struct PassManager {
  cache: gecko::cache::Cache,
  thunks: std::collections::VecDeque<Box<PassAction>>,
  global_scopes:
    std::collections::HashMap<gecko::name_resolution::Qualifier, gecko::name_resolution::Scope>,
  type_cache: gecko::type_inference::TypeCache,
}

impl PassManager {
  pub fn new() -> Self {
    PassManager {
      cache: gecko::cache::Cache::new(),
      thunks: std::collections::VecDeque::new(),
      global_scopes: std::collections::HashMap::new(),
      type_cache: gecko::type_inference::TypeCache::new(),
    }
  }

  fn name_resolution_decl(
    &mut self,
    module_qualifier: gecko::name_resolution::Qualifier,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut name_res_decl =
      gecko::name_resolution::NameResDeclContext::new(module_qualifier, &mut self.cache);

    name_res_decl.dispatch(&root_node);

    name_res_decl.diagnostics
  }

  fn name_resolution_link(
    &mut self,
    module_qualifier: gecko::name_resolution::Qualifier,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut name_res_link =
      gecko::name_resolution::NameResLinkContext::new(&self.global_scopes, &mut self.cache);

    name_res_link.dispatch(&root_node);

    name_res_link.diagnostics
  }

  fn type_inference(
    &mut self,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut type_inference =
      gecko::type_inference::TypeInferenceContext::new(&self.cache, &mut self.type_cache);

    gecko::visitor::traverse(root_node, &mut type_inference);

    type_inference.solve_constrains();

    type_inference.diagnostics
  }

  fn analysis(
    &mut self,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut type_check = gecko::type_check::TypeCheckContext::new(&self.cache);
    let mut lint = gecko::lint::LintContext::new();

    let mut aggregate_visitor = gecko::visitor::AggregateVisitor {
      visitors: vec![&mut type_check, &mut lint],
    };

    gecko::visitor::traverse(root_node, &mut aggregate_visitor);

    type_check
      .diagnostics
      .into_iter()
      .chain(lint.diagnostics)
      .collect()
  }

  fn then(&mut self, thunk: Box<PassAction>) -> &mut Self {
    self.thunks.push_back(thunk);

    self
  }

  fn run(&mut self) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut aggregated_diagnostics = Vec::new();

    while let Some(thunk) = self.thunks.pop_front() {
      let diagnostics = thunk();

      let break_flag = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == codespan_reporting::diagnostic::Severity::Error);

      aggregated_diagnostics.extend(diagnostics);

      if break_flag {
        break;
      }
    }

    return aggregated_diagnostics;
  }
}

fn test_passes() {
  let mut pass_manager = PassManager::new();
  let mut root_node = std::rc::Rc::new(todo!());
  let mut module_qualifier: gecko::name_resolution::Qualifier = todo!();

  pass_manager
    .then(Box::new(|| {
      pass_manager.name_resolution_decl(module_qualifier.clone(), root_node);

      todo!()
    }))
    .then(Box::new(|| {
      pass_manager.name_resolution_decl(module_qualifier.clone(), root_node);

      todo!()
    }));

  pass_manager.run();
}
