//! Group tree management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashMap;

use super::config::SortOrder;
use super::Instance;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(skip)]
    pub children: Vec<Group>,
}

impl Group {
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            collapsed: false,
            children: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GroupTree {
    roots: Vec<Group>,
    groups_by_path: HashMap<String, Group>,
    /// Tracks the first-seen insertion order of group paths (used as a stable base for other sorts).
    insertion_order: Vec<String>,
}

impl GroupTree {
    pub fn new_with_groups(instances: &[Instance], existing_groups: &[Group]) -> Self {
        let mut tree = Self {
            roots: Vec::new(),
            groups_by_path: HashMap::new(),
            insertion_order: Vec::new(),
        };

        // Add existing groups in the order they appear on disk (preserves prior save order)
        for group in existing_groups {
            tree.groups_by_path
                .insert(group.path.clone(), group.clone());
            tree.insertion_order.push(group.path.clone());
        }

        // Ensure all instance groups exist
        for inst in instances {
            if !inst.group_path.is_empty() {
                tree.ensure_group_exists(&inst.group_path);
            }
        }

        // Build tree structure
        tree.rebuild_tree();

        tree
    }

    fn ensure_group_exists(&mut self, path: &str) {
        if self.groups_by_path.contains_key(path) {
            return;
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
                self.insertion_order.push(current_path.clone());
            }
        }
    }

    fn rebuild_tree(&mut self) {
        self.roots.clear();

        // Build root groups in insertion order (no '/' in path); flatten_tree applies sort order.
        let root_paths: Vec<String> = self
            .insertion_order
            .iter()
            .filter(|p| self.groups_by_path.contains_key(*p) && !p.contains('/'))
            .cloned()
            .collect();

        let mut root_groups: Vec<Group> = root_paths
            .iter()
            .filter_map(|p| self.groups_by_path.get(p).cloned())
            .collect();

        for root in &mut root_groups {
            self.build_children(root);
        }

        self.roots = root_groups;
    }

    fn build_children(&self, parent: &mut Group) {
        let prefix = format!("{}/", parent.path);

        // Build children in insertion order
        let child_paths: Vec<String> = self
            .insertion_order
            .iter()
            .filter(|p| {
                self.groups_by_path.contains_key(*p)
                    && p.starts_with(&prefix)
                    && !p[prefix.len()..].contains('/')
            })
            .cloned()
            .collect();

        let mut children: Vec<Group> = child_paths
            .iter()
            .filter_map(|p| self.groups_by_path.get(p).cloned())
            .collect();

        for child in &mut children {
            self.build_children(child);
        }

        parent.children = children;
    }

    pub fn create_group(&mut self, path: &str) {
        self.ensure_group_exists(path);
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

        for p in &to_remove {
            self.groups_by_path.remove(p);
        }
        self.insertion_order.retain(|p| !to_remove.contains(p));

        self.rebuild_tree();
    }

    pub fn group_exists(&self, path: &str) -> bool {
        self.groups_by_path.contains_key(path)
    }

    pub fn get_all_groups(&self) -> Vec<Group> {
        // Return in insertion order so groups.json preserves creation order
        self.insertion_order
            .iter()
            .filter_map(|p| self.groups_by_path.get(p).cloned())
            .collect()
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

    pub fn set_collapsed(&mut self, path: &str, collapsed: bool) {
        if let Some(group) = self.groups_by_path.get_mut(path) {
            if group.collapsed != collapsed {
                group.collapsed = collapsed;
                self.rebuild_tree();
            }
        }
    }

    /// Rename a group and all its descendants to a new path.
    /// If the target path already exists, the old group is merged into it.
    pub fn rename_group(&mut self, old_path: &str, new_path: &str) {
        if old_path == new_path || new_path.is_empty() {
            return;
        }

        let old_prefix = format!("{}/", old_path);

        // Collect all paths to rename: the group itself + descendants
        let paths_to_rename: Vec<String> = self
            .insertion_order
            .iter()
            .filter(|p| *p == old_path || p.starts_with(&old_prefix))
            .cloned()
            .collect();

        for old in &paths_to_rename {
            let new = if *old == old_path {
                new_path.to_string()
            } else {
                format!("{}{}", new_path, &old[old_path.len()..])
            };

            if let Some(mut group) = self.groups_by_path.remove(old) {
                if self.groups_by_path.contains_key(&new) {
                    // Target exists: merge (keep existing, drop old)
                } else {
                    // Derive new name from the last path segment
                    let new_name = new.rsplit('/').next().unwrap_or(&new).to_string();
                    group.name = new_name;
                    group.path = new.clone();
                    self.groups_by_path.insert(new.clone(), group);
                }
            }

            // Update insertion_order: replace old with new, or remove if merged
            if let Some(pos) = self.insertion_order.iter().position(|p| p == old) {
                if self.insertion_order.contains(&new) {
                    // Target already in order list (merge case)
                    self.insertion_order.remove(pos);
                } else {
                    self.insertion_order[pos] = new;
                }
            }
        }

        // Ensure all parent groups of new_path exist
        self.ensure_group_exists(new_path);

        self.rebuild_tree();
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
        /// Which profile this group belongs to (set in all-profiles mode)
        profile: Option<String>,
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

fn sort_by_name<T, F>(items: &mut [T], sort_order: SortOrder, key: F)
where
    F: Fn(&T) -> &str,
{
    match sort_order {
        SortOrder::AZ => items.sort_by_key(|a| key(a).to_lowercase()),
        SortOrder::ZA => items.sort_by_key(|b| std::cmp::Reverse(key(b).to_lowercase())),
        SortOrder::Newest | SortOrder::Oldest | SortOrder::LastActivity => {}
    }
}

/// Sort a slice of session references by `sort_order`.
fn sort_sessions(sessions: &mut [&Instance], sort_order: SortOrder) {
    match sort_order {
        SortOrder::Oldest => sessions.sort_by_key(|i| i.created_at),
        SortOrder::Newest => sessions.sort_by_key(|i| Reverse(i.created_at)),
        SortOrder::LastActivity => sessions.sort_by_key(|i| last_activity_session_key(i)),
        SortOrder::AZ | SortOrder::ZA => sort_by_name(sessions, sort_order, |i| &i.title),
    }
}

/// Sort a slice of group references by `sort_order`, using `instances` for
/// timestamp-based orderings.
fn sort_groups<T, N, P>(
    items: &mut [T],
    sort_order: SortOrder,
    instances: &[Instance],
    name: N,
    path: P,
) where
    N: Fn(&T) -> &str,
    P: Fn(&T) -> &str,
{
    match sort_order {
        SortOrder::Oldest => {
            items.sort_by_key(|g| min_created_at_in_group(path(g), instances));
        }
        SortOrder::Newest => {
            items.sort_by_key(|g| Reverse(max_created_at_in_group(path(g), instances)));
        }
        SortOrder::LastActivity => {
            items.sort_by_key(|g| last_activity_group_key(path(g), instances));
        }
        SortOrder::AZ | SortOrder::ZA => sort_by_name(items, sort_order, name),
    }
}

/// Get the most recent created_at among all sessions (direct and nested) in a group.
/// Returns DateTime::MIN_UTC if the group has no sessions.
fn max_created_at_in_group(path: &str, instances: &[Instance]) -> DateTime<Utc> {
    let prefix = format!("{}/", path);
    instances
        .iter()
        .filter(|i| i.group_path == path || i.group_path.starts_with(&prefix))
        .map(|i| i.created_at)
        .max()
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

/// Get the oldest created_at among all sessions (direct and nested) in a group.
/// Returns DateTime::MAX_UTC if the group has no sessions (so empty groups sink to the bottom).
fn min_created_at_in_group(path: &str, instances: &[Instance]) -> DateTime<Utc> {
    let prefix = format!("{}/", path);
    instances
        .iter()
        .filter(|i| i.group_path == path || i.group_path.starts_with(&prefix))
        .map(|i| i.created_at)
        .min()
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// Get the most recent last_accessed_at among all sessions (direct and nested) in a group.
/// Groups with no sessions (or whose sessions have never reported activity) sort to the bottom
/// for descending order.
fn max_last_accessed_in_group(path: &str, instances: &[Instance]) -> Option<DateTime<Utc>> {
    let prefix = format!("{}/", path);
    instances
        .iter()
        .filter(|i| i.group_path == path || i.group_path.starts_with(&prefix))
        .filter_map(|i| i.last_accessed_at)
        .max()
}

/// Key used to sort sessions by LastActivity in descending order, pushing
/// sessions with no recorded activity to the bottom.
///
/// Rust's default ordering on `Option` places `None` BEFORE `Some(..)`; we
/// invert by wrapping in `Reverse` AND bucketing `None` into the "has no
/// activity" tier via the leading bool.
fn last_activity_session_key(inst: &Instance) -> (bool, Reverse<Option<DateTime<Utc>>>) {
    (
        inst.last_accessed_at.is_none(),
        Reverse(inst.last_accessed_at),
    )
}

/// Key used to sort groups by LastActivity in descending order. Groups with no
/// activity sort to the bottom.
fn last_activity_group_key(
    path: &str,
    instances: &[Instance],
) -> (bool, Reverse<Option<DateTime<Utc>>>) {
    let ts = max_last_accessed_in_group(path, instances);
    (ts.is_none(), Reverse(ts))
}

/// Flatten instances from multiple profiles into a single flat list.
/// Merges all profiles' sessions and groups at depth 0 (no profile headers).
/// Uses per-profile GroupTrees so collapsed state is isolated per profile.
pub fn flatten_tree_all_profiles(
    instances: &[Instance],
    group_trees: &std::collections::HashMap<String, GroupTree>,
    sort_order: SortOrder,
) -> Vec<Item> {
    let mut items = Vec::new();

    // Collect all ungrouped sessions across all profiles
    let mut ungrouped: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.group_path.is_empty())
        .collect();

    sort_sessions(&mut ungrouped, sort_order);

    for inst in ungrouped {
        items.push(Item::Session {
            id: inst.id.clone(),
            depth: 0,
        });
    }

    // Collect and flatten groups from all profiles at depth 0
    let mut all_roots: Vec<(&str, &Group, Vec<Instance>)> = Vec::new();
    for (profile_name, tree) in group_trees {
        let profile_instances: Vec<Instance> = instances
            .iter()
            .filter(|i| i.source_profile == *profile_name)
            .cloned()
            .collect();
        for root in tree.get_roots() {
            all_roots.push((profile_name, root, profile_instances.clone()));
        }
    }

    // Sort using the per-profile instances stored in each tuple (element 2),
    // not the global instances slice, so groups from different profiles with
    // the same name get sort keys scoped to their own profile's sessions.
    match sort_order {
        SortOrder::Oldest => {
            all_roots.sort_by_key(|(_, g, insts)| min_created_at_in_group(&g.path, insts));
        }
        SortOrder::Newest => {
            all_roots.sort_by_key(|(_, g, insts)| Reverse(max_created_at_in_group(&g.path, insts)));
        }
        SortOrder::LastActivity => {
            all_roots.sort_by_key(|(_, g, insts)| last_activity_group_key(&g.path, insts));
        }
        SortOrder::AZ | SortOrder::ZA => {
            sort_by_name(&mut all_roots, sort_order, |(_, g, _)| &*g.name)
        }
    }

    for (profile_name, root, profile_instances) in &all_roots {
        flatten_group(
            root,
            profile_instances,
            &mut items,
            0,
            sort_order,
            Some(profile_name),
        );
    }

    items
}

pub fn flatten_tree(
    group_tree: &GroupTree,
    instances: &[Instance],
    sort_order: SortOrder,
) -> Vec<Item> {
    let mut items = Vec::new();

    // Add ungrouped sessions first (always at top, sorted if needed)
    let mut ungrouped: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.group_path.is_empty())
        .collect();

    sort_sessions(&mut ungrouped, sort_order);

    for inst in ungrouped {
        items.push(Item::Session {
            id: inst.id.clone(),
            depth: 0,
        });
    }

    // Add groups and their sessions
    let roots = group_tree.get_roots();
    let mut roots_to_iterate: Vec<&Group> = roots.iter().collect();
    sort_groups(
        &mut roots_to_iterate,
        sort_order,
        instances,
        |g| &g.name,
        |g| &g.path,
    );

    for root in roots_to_iterate {
        flatten_group(root, instances, &mut items, 0, sort_order, None);
    }

    items
}

fn flatten_group(
    group: &Group,
    instances: &[Instance],
    items: &mut Vec<Item>,
    depth: usize,
    sort_order: SortOrder,
    profile: Option<&str>,
) {
    let session_count = count_sessions_in_group(&group.path, instances);

    items.push(Item::Group {
        path: group.path.clone(),
        name: group.name.clone(),
        depth,
        collapsed: group.collapsed,
        session_count,
        profile: profile.map(|s| s.to_string()),
    });

    if group.collapsed {
        return;
    }

    // Add sessions in this group (direct children only), sorted if needed
    let mut group_sessions: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.group_path == group.path)
        .collect();

    sort_sessions(&mut group_sessions, sort_order);

    for inst in group_sessions {
        items.push(Item::Session {
            id: inst.id.clone(),
            depth: depth + 1,
        });
    }

    // Recursively add child groups (sort them if needed)
    let mut children_to_iterate: Vec<&Group> = group.children.iter().collect();
    sort_groups(
        &mut children_to_iterate,
        sort_order,
        instances,
        |g| &g.name,
        |g| &g.path,
    );

    for child in children_to_iterate {
        flatten_group(child, instances, items, depth + 1, sort_order, profile);
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
        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);

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

        let items_expanded = flatten_tree(&tree, &instances, SortOrder::Oldest);
        let session_count_expanded = items_expanded
            .iter()
            .filter(|i| matches!(i, Item::Session { .. }))
            .count();
        assert_eq!(session_count_expanded, 1);

        tree.toggle_collapsed("work");
        let items_collapsed = flatten_tree(&tree, &instances, SortOrder::Oldest);
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
        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);

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

        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
        if let Some(Item::Group { collapsed, .. }) = items
            .iter()
            .find(|i| matches!(i, Item::Group { path, .. } if path == "work"))
        {
            assert!(!collapsed);
        }

        tree.toggle_collapsed("work");
        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
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

        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
        let group_count = items
            .iter()
            .filter(|i| matches!(i, Item::Group { .. }))
            .count();
        assert_eq!(group_count, 2);

        tree.toggle_collapsed("parent");
        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
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

        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
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
        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);

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
    fn test_delete_group_removes_from_insertion_order() {
        let mut inst1 = Instance::new("alpha-session", "/tmp/a");
        inst1.group_path = "alpha".to_string();
        let mut inst2 = Instance::new("beta-session", "/tmp/b");
        inst2.group_path = "beta".to_string();
        let mut inst3 = Instance::new("gamma-session", "/tmp/g");
        inst3.group_path = "gamma".to_string();
        let instances = vec![inst1, inst2, inst3];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        let initial_groups_vec = tree.get_all_groups();
        let initial_groups: Vec<_> = initial_groups_vec.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(initial_groups, vec!["alpha", "beta", "gamma"]);

        tree.delete_group("beta");

        let after_delete_vec = tree.get_all_groups();
        let after_delete: Vec<_> = after_delete_vec.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(after_delete, vec!["alpha", "gamma"]);

        tree.create_group("zeta");

        let after_create_vec = tree.get_all_groups();
        let after_create: Vec<_> = after_create_vec.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(after_create, vec!["alpha", "gamma", "zeta"]);
    }

    #[test]
    fn test_group_sort_order_in_flatten_tree() {
        // Groups are created in order: zebra, apple, mango (by instance order)
        let mut inst1 = Instance::new("z-session", "/tmp/z");
        inst1.group_path = "zebra".to_string();
        let mut inst2 = Instance::new("a-session", "/tmp/a");
        inst2.group_path = "apple".to_string();
        let mut inst3 = Instance::new("m-session", "/tmp/m");
        inst3.group_path = "mango".to_string();
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        // SortOrder::Oldest: groups sorted by oldest session (zebra, apple, mango)
        let items_oldest = flatten_tree(&tree, &instances, SortOrder::Oldest);
        let group_names_none: Vec<_> = items_oldest
            .iter()
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(group_names_none, vec!["zebra", "apple", "mango"]);

        // SortOrder::AZ: groups appear alphabetically
        let items_az = flatten_tree(&tree, &instances, SortOrder::AZ);
        let group_names_az: Vec<_> = items_az
            .iter()
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(group_names_az, vec!["apple", "mango", "zebra"]);

        // SortOrder::ZA: groups appear reverse alphabetically
        let items_za = flatten_tree(&tree, &instances, SortOrder::ZA);
        let group_names_za: Vec<_> = items_za
            .iter()
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(group_names_za, vec!["zebra", "mango", "apple"]);
    }

    #[test]
    fn test_sort_order_cycle() {
        assert_eq!(SortOrder::Newest.cycle(), SortOrder::LastActivity);
        assert_eq!(SortOrder::LastActivity.cycle(), SortOrder::Oldest);
        assert_eq!(SortOrder::Oldest.cycle(), SortOrder::AZ);
        assert_eq!(SortOrder::AZ.cycle(), SortOrder::ZA);
        assert_eq!(SortOrder::ZA.cycle(), SortOrder::Newest);
    }

    #[test]
    fn test_sort_order_cycle_reverse() {
        assert_eq!(SortOrder::Newest.cycle_reverse(), SortOrder::ZA);
        assert_eq!(SortOrder::ZA.cycle_reverse(), SortOrder::AZ);
        assert_eq!(SortOrder::AZ.cycle_reverse(), SortOrder::Oldest);
        assert_eq!(SortOrder::Oldest.cycle_reverse(), SortOrder::LastActivity);
        assert_eq!(SortOrder::LastActivity.cycle_reverse(), SortOrder::Newest);
    }

    #[test]
    fn test_sort_last_activity_descending_with_none_last() {
        use chrono::Duration;
        let now = Utc::now();
        let mut inst_recent = Instance::new("recent", "/tmp/r");
        inst_recent.last_accessed_at = Some(now);
        let mut inst_older = Instance::new("older", "/tmp/o");
        inst_older.last_accessed_at = Some(now - Duration::hours(1));
        let inst_never = Instance::new("never", "/tmp/n");
        let instances = vec![inst_never, inst_older, inst_recent];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::LastActivity);
        let titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, vec!["recent", "older", "never"]);
    }

    #[test]
    fn test_ungrouped_session_sort_oldest_preserves_insertion_order() {
        let inst1 = Instance::new("Mango", "/tmp/m");
        let inst2 = Instance::new("Apple", "/tmp/a");
        let inst3 = Instance::new("Zebra", "/tmp/z");
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
        let session_titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(session_titles, vec!["Mango", "Apple", "Zebra"]);
    }

    #[test]
    fn test_ungrouped_session_sort_az() {
        let inst1 = Instance::new("Mango", "/tmp/m");
        let inst2 = Instance::new("Apple", "/tmp/a");
        let inst3 = Instance::new("Zebra", "/tmp/z");
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::AZ);
        let session_titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(session_titles, vec!["Apple", "Mango", "Zebra"]);
    }

    #[test]
    fn test_ungrouped_session_sort_za() {
        let inst1 = Instance::new("Mango", "/tmp/m");
        let inst2 = Instance::new("Apple", "/tmp/a");
        let inst3 = Instance::new("Zebra", "/tmp/z");
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::ZA);
        let session_titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(session_titles, vec!["Zebra", "Mango", "Apple"]);
    }

    #[test]
    fn test_session_sort_oldest_within_group_preserves_insertion_order() {
        let mut inst1 = Instance::new("Mango", "/tmp/m");
        inst1.group_path = "work".to_string();
        let mut inst2 = Instance::new("Apple", "/tmp/a");
        inst2.group_path = "work".to_string();
        let mut inst3 = Instance::new("Zebra", "/tmp/z");
        inst3.group_path = "work".to_string();
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::Oldest);
        let session_titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(session_titles, vec!["Mango", "Apple", "Zebra"]);
    }

    #[test]
    fn test_session_sort_az_within_group() {
        let mut inst1 = Instance::new("Mango", "/tmp/m");
        inst1.group_path = "work".to_string();
        let mut inst2 = Instance::new("Apple", "/tmp/a");
        inst2.group_path = "work".to_string();
        let mut inst3 = Instance::new("Zebra", "/tmp/z");
        inst3.group_path = "work".to_string();
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::AZ);
        let session_titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(session_titles, vec!["Apple", "Mango", "Zebra"]);
    }

    #[test]
    fn test_session_sort_za_within_group() {
        let mut inst1 = Instance::new("Mango", "/tmp/m");
        inst1.group_path = "work".to_string();
        let mut inst2 = Instance::new("Apple", "/tmp/a");
        inst2.group_path = "work".to_string();
        let mut inst3 = Instance::new("Zebra", "/tmp/z");
        inst3.group_path = "work".to_string();
        let instances = vec![inst1, inst2, inst3];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::ZA);
        let session_titles: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Session { id, .. } => instances
                    .iter()
                    .find(|inst| &inst.id == id)
                    .map(|inst| inst.title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(session_titles, vec!["Zebra", "Mango", "Apple"]);
    }

    #[test]
    fn test_nested_child_groups_sort_order() {
        let mut inst_parent = Instance::new("parent-session", "/tmp/parent");
        inst_parent.group_path = "parent".to_string();
        let mut inst_zeta = Instance::new("zeta-session", "/tmp/zeta");
        inst_zeta.group_path = "parent/zeta".to_string();
        let mut inst_alpha = Instance::new("alpha-session", "/tmp/alpha");
        inst_alpha.group_path = "parent/alpha".to_string();
        let instances = vec![inst_parent, inst_zeta, inst_alpha];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items_oldest = flatten_tree(&tree, &instances, SortOrder::Oldest);
        let child_names_oldest: Vec<_> = items_oldest
            .iter()
            .skip(1)
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(child_names_oldest, vec!["zeta", "alpha"]);

        let items_az = flatten_tree(&tree, &instances, SortOrder::AZ);
        let child_names_az: Vec<_> = items_az
            .iter()
            .skip(1)
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(child_names_az, vec!["alpha", "zeta"]);

        let items_za = flatten_tree(&tree, &instances, SortOrder::ZA);
        let child_names_za: Vec<_> = items_za
            .iter()
            .skip(1)
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(child_names_za, vec!["zeta", "alpha"]);
    }

    #[test]
    fn test_sort_az_is_case_insensitive() {
        let mut inst1 = Instance::new("z-session", "/tmp/z");
        inst1.group_path = "Zebra".to_string();
        let mut inst2 = Instance::new("a-session", "/tmp/a");
        inst2.group_path = "apple".to_string();
        let instances = vec![inst1, inst2];
        let tree = GroupTree::new_with_groups(&instances, &[]);

        let items = flatten_tree(&tree, &instances, SortOrder::AZ);
        let group_names: Vec<_> = items
            .iter()
            .filter_map(|i| match i {
                Item::Group { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(group_names, vec!["apple", "Zebra"]);
    }

    #[test]
    fn test_existing_groups_vec_order_preserved_on_load() {
        let gamma_group = Group::new("gamma", "gamma");
        let alpha_group = Group::new("alpha", "alpha");
        let existing_groups = vec![gamma_group, alpha_group];

        let instances: Vec<Instance> = vec![];
        let tree = GroupTree::new_with_groups(&instances, &existing_groups);

        let roots = tree.get_roots();
        let root_names: Vec<_> = roots.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(root_names, vec!["gamma", "alpha"]);

        let all_groups: Vec<_> = tree
            .get_all_groups()
            .into_iter()
            .map(|g| g.name.as_str().to_string())
            .collect();
        assert_eq!(all_groups, vec!["gamma".to_string(), "alpha".to_string()]);
    }

    #[test]
    fn test_rename_group_simple() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.rename_group("work", "projects");

        assert!(!tree.group_exists("work"));
        assert!(tree.group_exists("projects"));
        assert_eq!(
            tree.groups_by_path.get("projects").unwrap().name,
            "projects"
        );
    }

    #[test]
    fn test_rename_group_with_children() {
        let mut inst1 = Instance::new("test1", "/tmp/1");
        inst1.group_path = "work".to_string();
        let mut inst2 = Instance::new("test2", "/tmp/2");
        inst2.group_path = "work/frontend".to_string();
        let instances = vec![inst1, inst2];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.rename_group("work", "projects");

        assert!(!tree.group_exists("work"));
        assert!(!tree.group_exists("work/frontend"));
        assert!(tree.group_exists("projects"));
        assert!(tree.group_exists("projects/frontend"));
    }

    #[test]
    fn test_rename_group_merge_into_existing() {
        let mut inst1 = Instance::new("test1", "/tmp/1");
        inst1.group_path = "old".to_string();
        let mut inst2 = Instance::new("test2", "/tmp/2");
        inst2.group_path = "existing".to_string();
        let instances = vec![inst1, inst2];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.rename_group("old", "existing");

        assert!(!tree.group_exists("old"));
        assert!(tree.group_exists("existing"));
    }

    #[test]
    fn test_rename_group_noop_same_path() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.rename_group("work", "work");

        assert!(tree.group_exists("work"));
    }

    #[test]
    fn test_rename_group_noop_empty_target() {
        let mut inst = Instance::new("test", "/tmp/t");
        inst.group_path = "work".to_string();
        let instances = vec![inst];
        let mut tree = GroupTree::new_with_groups(&instances, &[]);

        tree.rename_group("work", "");

        assert!(tree.group_exists("work"));
    }
}
