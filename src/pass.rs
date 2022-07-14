use gecko::visitor::{AnalysisVisitor, LoweringVisitor};

pub type PassAction =
  dyn FnOnce(&mut PassManager) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>>;

pub struct PassManager {
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

  pub fn add_name_resolution_decl(
    &mut self,
    module_qualifier: gecko::name_resolution::Qualifier,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut name_res_decl =
      gecko::name_resolution::NameResDeclContext::new(module_qualifier, &mut self.cache);

    name_res_decl.dispatch(&root_node);

    name_res_decl.diagnostics
  }

  pub fn add_name_resolution_link(
    &mut self,
    module_qualifier: gecko::name_resolution::Qualifier,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut name_res_link =
      gecko::name_resolution::NameResLinkContext::new(&self.global_scopes, &mut self.cache);

    name_res_link.dispatch(&root_node);

    name_res_link.diagnostics
  }

  pub fn add_type_inference(
    &mut self,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut type_inference =
      gecko::type_inference::TypeInferenceContext::new(&self.cache, &mut self.type_cache);

    gecko::visitor::traverse(root_node, &mut type_inference);

    type_inference.solve_constrains();

    type_inference.diagnostics
  }

  pub fn add_analysis(
    &self,
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

  pub fn add_lowering(
    &self,
    module_name: &str,
    root_node: std::rc::Rc<gecko::ast::Node>,
  ) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let llvm_context = inkwell::context::Context::create();
    let llvm_module = llvm_context.create_module(module_name);

    let mut lowering_context =
      gecko::lowering::LoweringContext::new(&self.cache, &llvm_context, &llvm_module);

    LoweringVisitor::dispatch(&mut lowering_context, &root_node);

    Vec::new()
  }

  pub fn run(&mut self) -> Vec<codespan_reporting::diagnostic::Diagnostic<usize>> {
    let mut aggregated_diagnostics = Vec::new();

    while let Some(thunk) = self.thunks.pop_front() {
      let diagnostics = thunk(self);

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
