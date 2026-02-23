//! Group tree management

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::Instance;

/// Returns the parent of a group path (everything before the last `/`),
/// or an empty string for root-level paths.
pub fn parent_path(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub sort_order: u32,
    #[serde(skip)]
    pub children: Vec<Group>,
}

impl Group {
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            collapsed: false,
            sort_order: 0,
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupTree {
    roots: Vec<Group>,
    groups_by_path: HashMap<String, Group>,
}

impl GroupTree {
    pub fn new_with_groups(instances: &[Instance], existing_groups: &[Group]) -> Self {
        let mut tree = Self {
            roots: Vec::new(),
            groups_by_path: HashMap::new(),
        };

        // Add existing groups
        for group in existing_groups {
            tree.groups_by_path
                .insert(group.path.clone(), group.clone());
        }

        // Ensure all instance groups exist
        for inst in instances {
            if !inst.group_path.is_empty() {
                tree.ensure_group_exists(&inst.group_path);
            }
        }

        // Normalize sort_orders (handles upgrade from all-zero/alphabetical)
        tree.initialize_sort_orders();

        // Build tree structure
        tree.rebuild_tree();

        tree
    }

    /// Returns the paths that were newly created (did not previously exist).
    fn ensure_group_exists(&mut self, path: &str) -> Vec<String> {
        let mut created = Vec::new();
        if self.groups_by_path.contains_key(path) {
            return created;
        }

        // Create all parent groups
        let parts: Vec<&str> = path.split('/').collect();
        let mut current_path = String::new();

        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                current_path.push('/');
            }
            current_path.push_str(part);

            if !self.groups_by_path.contains_key(&current_path) {
                let group = Group::new(part, &current_path);
                self.groups_by_path.insert(current_path.clone(), group);
                created.push(current_path.clone());
            }
        }
        created
    }

    /// Normalize sort_orders so siblings have contiguous values.
    /// Uses (sort_order, name) as tiebreaker so the first load after upgrade
    /// from all-zero preserves alphabetical order.
    fn initialize_sort_orders(&mut self) {
        // Collect all paths grouped by parent
        let mut by_parent: HashMap<String, Vec<String>> = HashMap::new();
        for path in self.groups_by_path.keys() {
            by_parent
                .entry(parent_path(path))
                .or_default()
                .push(path.clone());
        }

        // For each parent, sort siblings by (sort_order, name) and assign contiguous values
        for (_parent, mut siblings) in by_parent {
            siblings.sort_by(|a, b| {
                let ga = &self.groups_by_path[a];
                let gb = &self.groups_by_path[b];
                ga.sort_order
                    .cmp(&gb.sort_order)
                    .then(ga.name.cmp(&gb.name))
            });
            for (i, path) in siblings.iter().enumerate() {
                if let Some(g) = self.groups_by_path.get_mut(path) {
                    g.sort_order = i as u32;
                }
            }
        }
    }

    fn rebuild_tree(&mut self) {
        self.roots.clear();

        // Find root groups (no '/' in path)
        let mut root_groups: Vec<Group> = self
            .groups_by_path
            .values()
            .filter(|g| !g.path.contains('/'))
            .cloned()
            .collect();

        root_groups.sort_by_key(|g| g.sort_order);

        // Build children recursively
        for root in &mut root_groups {
            self.build_children(root);
        }

        self.roots = root_groups;
    }

    fn build_children(&self, parent: &mut Group) {
        let prefix = format!("{}/", parent.path);

        let mut children: Vec<Group> = self
            .groups_by_path
            .values()
            .filter(|g| g.path.starts_with(&prefix) && !g.path[prefix.len()..].contains('/'))
            .cloned()
            .collect();

        children.sort_by_key(|g| g.sort_order);

        for child in &mut children {
            self.build_children(child);
        }

        parent.children = children;
    }

    pub fn create_group(&mut self, path: &str) {
        let created = self.ensure_group_exists(path);

        // For each newly created path, bump existing siblings so the new group is at the top
        for new_path in &created {
            let parent = parent_path(new_path);

            for group in self.groups_by_path.values_mut() {
                if parent_path(&group.path) == parent && group.path != *new_path {
                    group.sort_order += 1;
                }
            }
            // New group stays at sort_order 0 (already the default from Group::new)
        }

        self.rebuild_tree();
    }

    pub fn delete_group(&mut self, path: &str) {
        // Remove group and all children
        let prefix = format!("{}/", path);
        let to_remove: Vec<String> = self
            .groups_by_path
            .keys()
            .filter(|p| *p == path || p.starts_with(&prefix))
            .cloned()
            .collect();

        for p in to_remove {
            self.groups_by_path.remove(&p);
        }

        self.rebuild_tree();
    }

    pub fn group_exists(&self, path: &str) -> bool {
        self.groups_by_path.contains_key(path)
    }

    pub fn get_all_groups(&self) -> Vec<Group> {
        let mut groups: Vec<Group> = self.groups_by_path.values().cloned().collect();
        groups.sort_by(|a, b| a.path.cmp(&b.path));
        groups
    }

    pub fn get_roots(&self) -> &[Group] {
        &self.roots
    }

    pub fn toggle_collapsed(&mut self, path: &str) {
        if let Some(group) = self.groups_by_path.get_mut(path) {
            group.collapsed = !group.collapsed;
            self.rebuild_tree();
        }
    }

    pub fn get_group_mut(&mut self, path: &str) -> Option<&mut Group> {
        self.groups_by_path.get_mut(path)
    }

    /// Returns sibling paths sorted by sort_order.
    pub fn get_sibling_paths(&self, path: &str) -> Vec<String> {
        let parent = parent_path(path);

        let mut siblings: Vec<&Group> = self
            .groups_by_path
            .values()
            .filter(|g| parent_path(&g.path) == parent)
            .collect();

        siblings.sort_by_key(|g| g.sort_order);
        siblings.iter().map(|g| g.path.clone()).collect()
    }

    /// Swap sort_order values of two sibling groups and rebuild tree.
    pub fn swap_group_order(&mut self, path_a: &str, path_b: &str) {
        let order_a = self.groups_by_path.get(path_a).map(|g| g.sort_order);
        let order_b = self.groups_by_path.get(path_b).map(|g| g.sort_order);

        if let (Some(a), Some(b)) = (order_a, order_b) {
            if let Some(g) = self.groups_by_path.get_mut(path_a) {
                g.sort_order = b;
            }
            if let Some(g) = self.groups_by_path.get_mut(path_b) {
                g.sort_order = a;
            }
            self.rebuild_tree();
        }
    }
}

/// Item represents either a group or an instance in the flattened tree view
#[derive(Debug, Clone)]
pub enum Item {
    Group {
        path: String,
        name: String,
        depth: usize,
        collapsed: bool,
        session_count: usize,
    },
    Session {
        id: String,
        depth: usize,
    },
}

impl Item {
    pub fn depth(&self) -> usize {
        match self {
            Item::Group { depth, .. } => *depth,
            Item::Session { depth, .. } => *depth,
        }
    }
}

pub fn flatten_tree(group_tree: &GroupTree, instances: &[Instance]) -> Vec<Item> {
    let mut items = Vec::new();

    // Add ungrouped sessions first
    let ungrouped: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.group_path.is_empty())
        .collect();

    for inst in ungrouped {
        items.push(Item::Session {
            id: inst.id.clone(),
            depth: 0,
        });
    }

    // Add groups and their sessions
    for root in group_tree.get_roots() {
        flatten_group(root, instances, &mut items, 0);
    }

    items
}

fn flatten_group(group: &Group, instances: &[Instance], items: &mut Vec<Item>, depth: usize) {
    let session_count = count_sessions_in_group(&group.path, instances);

    items.push(Item::Group {
        path: group.path.clone(),
        name: group.name.clone(),
        depth,
        collapsed: group.collapsed,
        session_count,
    });

    if group.collapsed {
        return;
    }

    // Add sessions in this group (direct children only)
    let group_sessions: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.group_path == group.path)
        .collect();

    for inst in group_sessions {
        items.push(Item::Session {
            id: inst.id.clone(),
            depth: depth + 1,
        });
    }

    // Recursively add child groups
    for child in &group.children {
        flatten_group(child, instances, items, depth + 1);
    }
}

fn count_sessions_in_group(path: &str, instances: &[Instance]) -> usize {
    let prefix = format!("{}/", path);
    instances
        .iter()
        .filter(|i| i.group_path == path || i.group_path.starts_with(&prefix))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_tree_creation() {
        let mut inst1 = Instance::new("test1", "/tmp/1");
        inst1.group_path = "work".to_string();
        let mut inst2 = Instance::new("test2", "/tmp/2");
        inst2.group_path = "work/frontend".to_string();
        let mut inst3 = Instance::new("test3", "/tmp/3");
        inst3.group_path = "personal".to_string();

        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        assert!(tree.group_exists("work"));
        assert!(tree.group_exists("work/frontend"));
        assert!(tree.group_exists("personal"));
        assert!(!tree.group_exists("nonexistent"));
    }

    #[test]
    fn test_flatten_tree() {
        let ungrouped = Instance::new("ungrouped", "/tmp/u");
        let mut inst1 = Instance::new("test1", "/tmp/1");
        inst1.group_path = "work".to_string();
        let mut inst2 = Instance::new("test2", "/tmp/2");
        inst2.group_path = "work".to_string();

        let instances = vec![ungrouped, inst1, inst2];
        let tree = GroupTree::new_with_groups(&instances, &[]);
        let items = flatten_tree(&tree, &instances);

        assert!(!items.is_empty());

        // First item should be ungrouped session
        assert!(matches!(items[0], Item::Session { .. }));
    }

    #[test]
    fn test_toggle_collapsed() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        let group = tree.groups_by_path.get("work").unwrap();
        assert!(!group.collapsed);

        tree.toggle_collapsed("work");

        let group = tree.groups_by_path.get("work").unwrap();
        assert!(group.collapsed);

        tree.toggle_collapsed("work");

        let group = tree.groups_by_path.get("work").unwrap();
        assert!(!group.collapsed);
    }

    #[test]
    fn test_toggle_collapsed_nonexistent_group() {
        let instances: Vec<Instance> = vec![];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);
        tree.toggle_collapsed("nonexistent");
    }

    #[test]
    fn test_collapsed_group_hides_sessions_in_flatten() {
        let mut inst1 = Instance::new("work-session", "/tmp/w");
        inst1.group_path = "work".to_string();
        let instances = vec![inst1];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        let items_expanded = flatten_tree(&tree, &instances);
        let session_count_expanded = items_expanded
            .iter()
            .filter(|i| matches!(i, Item::Session { .. }))
            .count();
        assert_eq!(session_count_expanded, 1);

        tree.toggle_collapsed("work");
        let items_collapsed = flatten_tree(&tree, &instances);
        let session_count_collapsed = items_collapsed
            .iter()
            .filter(|i| matches!(i, Item::Session { .. }))
            .count();
        assert_eq!(session_count_collapsed, 0);
    }

    #[test]
    fn test_collapsed_group_still_shows_in_flatten() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.toggle_collapsed("work");
        let items = flatten_tree(&tree, &instances);

        let group_items: Vec<_> = items
            .iter()
            .filter(|i| matches!(i, Item::Group { .. }))
            .collect();
        assert_eq!(group_items.len(), 1);
    }

    #[test]
    fn test_collapsed_state_in_flattened_item() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances);
        if let Some(Item::Group { collapsed, .. }) = items
            .iter()
            .find(|i| matches!(i, Item::Group { path, .. } if path == "work"))
        {
            assert!(!collapsed);
        }

        tree.toggle_collapsed("work");
        let items = flatten_tree(&tree, &instances);
        if let Some(Item::Group { collapsed, .. }) = items
            .iter()
            .find(|i| matches!(i, Item::Group { path, .. } if path == "work"))
        {
            assert!(*collapsed);
        }
    }

    #[test]
    fn test_nested_group_collapse_hides_children() {
        let mut inst1 = Instance::new("parent-session", "/tmp/p");
        inst1.group_path = "parent".to_string();
        let mut inst2 = Instance::new("child-session", "/tmp/c");
        inst2.group_path = "parent/child".to_string();
        let instances = vec![inst1, inst2];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances);
        let group_count = items
            .iter()
            .filter(|i| matches!(i, Item::Group { .. }))
            .count();
        assert_eq!(group_count, 2);

        tree.toggle_collapsed("parent");
        let items = flatten_tree(&tree, &instances);
        let group_count_collapsed = items
            .iter()
            .filter(|i| matches!(i, Item::Group { .. }))
            .count();
        assert_eq!(group_count_collapsed, 1);
    }

    #[test]
    fn test_session_count_includes_nested() {
        let mut inst1 = Instance::new("parent-session", "/tmp/p");
        inst1.group_path = "parent".to_string();
        let mut inst2 = Instance::new("child-session", "/tmp/c");
        inst2.group_path = "parent/child".to_string();
        let instances = vec![inst1, inst2];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances);
        if let Some(Item::Group { session_count, .. }) = items
            .iter()
            .find(|i| matches!(i, Item::Group { path, .. } if path == "parent"))
        {
            assert_eq!(*session_count, 2);
        }
    }

    #[test]
    fn test_delete_group() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        assert!(tree.group_exists("work"));
        tree.delete_group("work");
        assert!(!tree.group_exists("work"));
    }

    #[test]
    fn test_delete_group_removes_children() {
        let mut inst1 = Instance::new("parent-session", "/tmp/p");
        inst1.group_path = "parent".to_string();
        let mut inst2 = Instance::new("child-session", "/tmp/c");
        inst2.group_path = "parent/child".to_string();
        let instances = vec![inst1, inst2];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        assert!(tree.group_exists("parent"));
        assert!(tree.group_exists("parent/child"));

        tree.delete_group("parent");

        assert!(!tree.group_exists("parent"));
        assert!(!tree.group_exists("parent/child"));
    }

    #[test]
    fn test_create_group() {
        let instances: Vec<Instance> = vec![];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        assert!(!tree.group_exists("new-group"));
        tree.create_group("new-group");
        assert!(tree.group_exists("new-group"));
    }

    #[test]
    fn test_create_nested_group_creates_parents() {
        let instances: Vec<Instance> = vec![];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.create_group("a/b/c");
        assert!(tree.group_exists("a"));
        assert!(tree.group_exists("a/b"));
        assert!(tree.group_exists("a/b/c"));
    }

    #[test]
    fn test_item_depth() {
        let ungrouped = Instance::new("ungrouped", "/tmp/u");
        let mut inst1 = Instance::new("root-level", "/tmp/r");
        inst1.group_path = "root".to_string();
        let mut inst2 = Instance::new("nested", "/tmp/n");
        inst2.group_path = "root/child".to_string();
        let instances = vec![ungrouped, inst1, inst2];
        let tree = GroupTree::new_with_groups(&instances, &[]);
        let items = flatten_tree(&tree, &instances);

        for item in &items {
            match item {
                Item::Session { id, depth } if !id.is_empty() => {
                    if *depth == 0 {
                        continue;
                    }
                    assert!(*depth >= 1);
                }
                Item::Group { path, depth, .. } => {
                    if path == "root" {
                        assert_eq!(*depth, 0);
                    } else if path == "root/child" {
                        assert_eq!(*depth, 1);
                    }
                }
                _ => {}
            }
        }
    }

    #[test]
    fn test_get_roots_returns_only_top_level() {
        let mut inst1 = Instance::new("test1", "/tmp/1");
        inst1.group_path = "alpha".to_string();
        let mut inst2 = Instance::new("test2", "/tmp/2");
        inst2.group_path = "alpha/nested".to_string();
        let mut inst3 = Instance::new("test3", "/tmp/3");
        inst3.group_path = "beta".to_string();
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let roots = tree.get_roots();
        assert_eq!(roots.len(), 2);

        let root_names: Vec<_> = roots.iter().map(|g| &g.name).collect();
        assert!(root_names.contains(&&"alpha".to_string()));
        assert!(root_names.contains(&&"beta".to_string()));
    }

    #[test]
    fn test_groups_initial_sort_order_uses_alphabetical_tiebreaker() {
        // When all groups have sort_order 0 (e.g., after upgrade), initialize_sort_orders
        // uses alphabetical tiebreaker to assign contiguous values
        let mut inst1 = Instance::new("z-session", "/tmp/z");
        inst1.group_path = "zebra".to_string();
        let mut inst2 = Instance::new("a-session", "/tmp/a");
        inst2.group_path = "apple".to_string();
        let mut inst3 = Instance::new("m-session", "/tmp/m");
        inst3.group_path = "mango".to_string();
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let roots = tree.get_roots();
        assert_eq!(roots[0].name, "apple");
        assert_eq!(roots[1].name, "mango");
        assert_eq!(roots[2].name, "zebra");
    }

    #[test]
    fn test_sort_order_preserved_across_rebuild() {
        let mut inst1 = Instance::new("z-session", "/tmp/z");
        inst1.group_path = "zebra".to_string();
        let mut inst2 = Instance::new("a-session", "/tmp/a");
        inst2.group_path = "apple".to_string();
        let instances = vec![inst1, inst2];

        // Build tree, then manually swap order
        let mut tree = GroupTree::new_with_groups(&instances, &[]);
        tree.swap_group_order("zebra", "apple");

        let roots = tree.get_roots();
        assert_eq!(roots[0].name, "zebra");
        assert_eq!(roots[1].name, "apple");
    }

    #[test]
    fn test_get_sibling_paths() {
        let mut inst1 = Instance::new("a-session", "/tmp/a");
        inst1.group_path = "alpha".to_string();
        let mut inst2 = Instance::new("b-session", "/tmp/b");
        inst2.group_path = "beta".to_string();
        let mut inst3 = Instance::new("c-session", "/tmp/c");
        inst3.group_path = "alpha/child1".to_string();
        let mut inst4 = Instance::new("d-session", "/tmp/d");
        inst4.group_path = "alpha/child2".to_string();
        let instances = vec![inst1, inst2, inst3, inst4];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        // Root siblings
        let root_siblings = tree.get_sibling_paths("alpha");
        assert_eq!(root_siblings, vec!["alpha", "beta"]);

        // Nested siblings
        let nested_siblings = tree.get_sibling_paths("alpha/child1");
        assert_eq!(nested_siblings, vec!["alpha/child1", "alpha/child2"]);
    }

    #[test]
    fn test_swap_group_order() {
        let mut inst1 = Instance::new("a-session", "/tmp/a");
        inst1.group_path = "alpha".to_string();
        let mut inst2 = Instance::new("b-session", "/tmp/b");
        inst2.group_path = "beta".to_string();
        let mut inst3 = Instance::new("c-session", "/tmp/c");
        inst3.group_path = "gamma".to_string();
        let instances = vec![inst1, inst2, inst3];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        // Initial: alpha(0), beta(1), gamma(2)
        assert_eq!(tree.get_roots()[0].name, "alpha");
        assert_eq!(tree.get_roots()[1].name, "beta");
        assert_eq!(tree.get_roots()[2].name, "gamma");

        // Swap alpha and beta
        tree.swap_group_order("alpha", "beta");
        assert_eq!(tree.get_roots()[0].name, "beta");
        assert_eq!(tree.get_roots()[1].name, "alpha");
        assert_eq!(tree.get_roots()[2].name, "gamma");
    }

    #[test]
    fn test_create_group_inserts_at_top() {
        let mut inst1 = Instance::new("a-session", "/tmp/a");
        inst1.group_path = "alpha".to_string();
        let mut inst2 = Instance::new("b-session", "/tmp/b");
        inst2.group_path = "beta".to_string();
        let instances = vec![inst1, inst2];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        // Create a new group; it should appear at the top
        tree.create_group("omega");
        let roots = tree.get_roots();
        assert_eq!(roots[0].name, "omega");
        assert_eq!(roots[1].name, "alpha");
        assert_eq!(roots[2].name, "beta");
    }

    #[test]
    fn test_initialize_sort_orders_idempotent() {
        let mut inst1 = Instance::new("a-session", "/tmp/a");
        inst1.group_path = "alpha".to_string();
        let mut inst2 = Instance::new("b-session", "/tmp/b");
        inst2.group_path = "beta".to_string();
        let instances = vec![inst1, inst2];

        let tree1 = GroupTree::new_with_groups(&instances, &[]);
        let order1: Vec<String> = tree1.get_roots().iter().map(|g| g.name.clone()).collect();

        // Build a second time with the same groups (simulating reload)
        let groups = tree1.get_all_groups();
        let tree2 = GroupTree::new_with_groups(&instances, &groups);
        let order2: Vec<String> = tree2.get_roots().iter().map(|g| g.name.clone()).collect();

        assert_eq!(order1, order2);
    }

    #[test]
    fn test_create_nested_group_inserts_at_top_of_siblings() {
        let mut inst1 = Instance::new("a-session", "/tmp/a");
        inst1.group_path = "parent/child_a".to_string();
        let mut inst2 = Instance::new("b-session", "/tmp/b");
        inst2.group_path = "parent/child_b".to_string();
        let instances = vec![inst1, inst2];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        // Create a new nested group under "parent"
        tree.create_group("parent/new_child");

        let roots = tree.get_roots();
        assert_eq!(roots.len(), 1);
        let parent = &roots[0];
        assert_eq!(parent.children[0].name, "new_child");
    }
}
