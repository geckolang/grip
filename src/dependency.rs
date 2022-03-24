use std::collections::vec_deque;

use crate::package;

type DependencyGraph = std::collections::HashMap<String, Vec<String>>;

fn build_dependency_graph(manifest: package::Manifest) -> Result<DependencyGraph, String> {
  let mut dependency_graph = DependencyGraph::new();
  let mut dependencies_queue = std::collections::VecDeque::from(manifest.dependencies);

  // REVISE: This isn't actually a queue. It's being popped, so its used as a stack.
  // ... This means that the search algorithm being used is breadth-first instead of
  // ... depth-first.
  while let Some(dependency_name) = dependencies_queue.pop_front() {
    let mut manifest_path = std::path::PathBuf::from(package::PATH_DEPENDENCIES);

    manifest_path.push(dependency_name.clone());
    manifest_path.push(package::PATH_MANIFEST_FILE);

    let dependencies = package::fetch_manifest(&manifest_path)?.dependencies;

    dependency_graph.insert(dependency_name, dependencies.clone());

    // TODO: Does this 'push_back' all the elements?
    // dependencies_queue.extend(dependencies);

    for dep in dependencies {
      dependencies_queue.push_back(dep);
    }
  }

  Ok(dependency_graph)
}

fn is_dependency_cyclic(dependency_graph: &DependencyGraph, dependency_name: String) -> bool {
  let mut visited = std::collections::HashSet::new();
  let mut queue = std::collections::VecDeque::new();

  queue.push_back(dependency_name);

  while let Some(dependency_name) = queue.pop_front() {
    if visited.contains(&dependency_name) {
      return true;
    }

    visited.insert(dependency_name.clone());

    if let Some(dependencies) = dependency_graph.get(&dependency_name) {
      queue.extend(dependencies.iter().cloned());
    }
  }

  false
}

fn find_most_used_dependency(dependency_graph: DependencyGraph) -> Option<String> {
  let mut most_used = None;
  let mut count_buffer = 0;

  for (dependency_name, dependencies) in dependency_graph {
    let dependency_count = dependencies.len();

    if dependency_count > count_buffer {
      most_used = Some(dependency_name);
      count_buffer = dependency_count;
    }
  }

  most_used
}
