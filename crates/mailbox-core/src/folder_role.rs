//! Classify IMAP mailboxes into system Folders vs user Labels (Gmail-style).

use crate::types::{is_snoozed_folder, Folder};

/// System mailbox role, or [`MailboxRole::User`] for custom labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MailboxRole {
    Inbox,
    Starred,
    Snoozed,
    Important,
    Sent,
    Drafts,
    Archive,
    Spam,
    Trash,
    /// Custom / user label (not a standard system mailbox).
    User,
}

impl MailboxRole {
    pub fn is_system(self) -> bool {
        !matches!(self, Self::User)
    }

    /// Nav sort among system folders (lower first). User labels use alphabetical path.
    pub fn sort_rank(self) -> u8 {
        match self {
            Self::Inbox => 0,
            Self::Starred => 1,
            Self::Snoozed => 2,
            Self::Important => 3,
            Self::Sent => 4,
            Self::Drafts => 5,
            Self::Archive => 6,
            Self::Spam => 7,
            Self::Trash => 8,
            Self::User => 9,
        }
    }
}

fn path_leaf(path: &str) -> &str {
    path.rsplit(['/', '.'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

/// Gmail “All Mail” — archive move target, hidden from the rail by default.
pub fn is_all_mail(path: &str, name: &str) -> bool {
    let p = path.to_lowercase();
    let n = name.to_lowercase();
    p.contains("[gmail]/all mail") || n == "all mail"
}

/// Classify a mailbox path/name into a system role or user label.
pub fn classify_mailbox(path: &str, name: &str) -> MailboxRole {
    if is_snoozed_folder(path) {
        return MailboxRole::Snoozed;
    }

    let p = path.to_lowercase();
    let n = name.to_lowercase();
    let leaf = path_leaf(&p).to_string();
    let gmail = p.contains("[gmail]");

    if p == "inbox" || leaf == "inbox" || n == "inbox" {
        return MailboxRole::Inbox;
    }
    if leaf == "starred" || n == "starred" || (gmail && p.contains("starred")) {
        return MailboxRole::Starred;
    }
    if leaf == "important" || n == "important" || (gmail && p.contains("important")) {
        return MailboxRole::Important;
    }
    if leaf.contains("sent") || n.contains("sent") {
        return MailboxRole::Sent;
    }
    if leaf.contains("draft") || n.contains("draft") {
        return MailboxRole::Drafts;
    }
    if leaf == "spam"
        || leaf == "junk"
        || n == "spam"
        || n == "junk"
        || (gmail && (p.contains("spam") || p.contains("junk")))
    {
        return MailboxRole::Spam;
    }
    if leaf.contains("trash")
        || leaf.contains("bin")
        || leaf.contains("deleted")
        || n.contains("trash")
        || n.contains("bin")
        || n.contains("deleted")
    {
        return MailboxRole::Trash;
    }
    if leaf.contains("archive")
        || n.contains("archive")
        || is_all_mail(path, name)
    {
        return MailboxRole::Archive;
    }

    MailboxRole::User
}

/// Display order key: system role rank, then path (case-insensitive).
pub fn folder_sort_key(path: &str, name: &str) -> (u8, String) {
    let role = classify_mailbox(path, name);
    (role.sort_rank(), path.to_lowercase())
}

pub fn sort_folders(folders: &mut [Folder]) {
    folders.sort_by_key(|a| folder_sort_key(&a.path, &a.name));
}

/// Guess hierarchy delimiter from a set of user-label paths.
pub fn label_delimiter(paths: &[&str]) -> char {
    let slash = paths.iter().any(|p| p.contains('/'));
    if slash {
        return '/';
    }
    let dot = paths.iter().filter(|p| p.matches('.').count() >= 1).count();
    // Prefer `.` only when several paths look hierarchical (Courier-style).
    if dot >= 2 {
        '.'
    } else {
        '/'
    }
}

/// Node in the Labels nav tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelNode {
    /// Raw path segment (IMAP wire form; used for tree identity).
    pub segment: String,
    /// Unicode label for the UI (Modified UTF-7 decoded / folder.name).
    pub display_name: String,
    /// Full IMAP path when this node is a real mailbox; `None` for synthetic parents.
    pub full_path: Option<String>,
    pub unread: u32,
    /// Own unread plus descendants.
    pub unread_sum: u32,
    pub children: Vec<LabelNode>,
}

impl LabelNode {
    pub fn is_selectable(&self) -> bool {
        self.full_path.is_some()
    }
}

/// Build a nested label tree from user-role folders (already filtered to [`MailboxRole::User`]).
pub fn build_label_tree(folders: &[Folder]) -> Vec<LabelNode> {
    let paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
    let delim = label_delimiter(&paths);
    let mut roots: Vec<LabelNode> = Vec::new();

    for folder in folders {
        let parts: Vec<&str> = folder
            .path
            .split(delim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }
        insert_label_path(
            &mut roots,
            &parts,
            0,
            &folder.path,
            &folder.name,
            folder.unread,
            delim,
        );
    }

    sort_label_nodes(&mut roots);
    recompute_unread_sums(&mut roots);
    roots
}

fn insert_label_path(
    nodes: &mut Vec<LabelNode>,
    parts: &[&str],
    depth: usize,
    full_path: &str,
    folder_name: &str,
    unread: u32,
    delim: char,
) {
    let Some(segment) = parts.get(depth).copied() else {
        return;
    };
    let is_leaf = depth + 1 == parts.len();

    let idx = if let Some(i) = nodes.iter().position(|n| n.segment == segment) {
        i
    } else {
        nodes.push(LabelNode {
            segment: segment.to_string(),
            // Intermediate parents: decode wire segment; leaf overridden below.
            display_name: crate::imap::decode_modified_utf7(segment),
            full_path: None,
            unread: 0,
            unread_sum: 0,
            children: Vec::new(),
        });
        nodes.len() - 1
    };

    if is_leaf {
        nodes[idx].full_path = Some(full_path.to_string());
        nodes[idx].unread = unread;
        // Prefer the store's friendly name (already UTF-7 decoded at LIST time).
        if !folder_name.is_empty() {
            nodes[idx].display_name = folder_name.to_string();
        } else {
            nodes[idx].display_name = crate::imap::decode_modified_utf7(segment);
        }
    } else {
        insert_label_path(
            &mut nodes[idx].children,
            parts,
            depth + 1,
            full_path,
            folder_name,
            unread,
            delim,
        );
    }
}

fn sort_label_nodes(nodes: &mut [LabelNode]) {
    nodes.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    for n in nodes.iter_mut() {
        sort_label_nodes(&mut n.children);
    }
}

fn recompute_unread_sums(nodes: &mut [LabelNode]) {
    for n in nodes.iter_mut() {
        recompute_unread_sums(&mut n.children);
        let child_sum: u32 = n.children.iter().map(|c| c.unread_sum).sum();
        n.unread_sum = n.unread.saturating_add(child_sum);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(path: &str, name: &str) -> Folder {
        Folder {
            account_id: "a".into(),
            name: name.into(),
            path: path.into(),
            unread: 0,
            hidden: false,
        }
    }

    #[test]
    fn classify_gmail_system_and_user() {
        assert_eq!(classify_mailbox("INBOX", "Inbox"), MailboxRole::Inbox);
        assert_eq!(
            classify_mailbox("[Gmail]/Sent Mail", "Sent Mail"),
            MailboxRole::Sent
        );
        assert_eq!(
            classify_mailbox("[Gmail]/Drafts", "Drafts"),
            MailboxRole::Drafts
        );
        assert_eq!(
            classify_mailbox("[Gmail]/Spam", "Spam"),
            MailboxRole::Spam
        );
        assert_eq!(
            classify_mailbox("[Gmail]/Trash", "Trash"),
            MailboxRole::Trash
        );
        assert_eq!(
            classify_mailbox("[Gmail]/Starred", "Starred"),
            MailboxRole::Starred
        );
        assert_eq!(
            classify_mailbox("[Gmail]/Important", "Important"),
            MailboxRole::Important
        );
        assert_eq!(
            classify_mailbox("[Gmail]/All Mail", "All Mail"),
            MailboxRole::Archive
        );
        assert!(is_all_mail("[Gmail]/All Mail", "All Mail"));
        assert_eq!(
            classify_mailbox("работа/foo", "foo"),
            MailboxRole::User
        );
        assert_eq!(classify_mailbox("bookmarks", "bookmarks"), MailboxRole::User);
    }

    #[test]
    fn classify_non_gmail() {
        assert_eq!(classify_mailbox("Sent", "Sent"), MailboxRole::Sent);
        assert_eq!(classify_mailbox("Junk", "Junk"), MailboxRole::Spam);
        assert_eq!(classify_mailbox("Drafts", "Drafts"), MailboxRole::Drafts);
        assert_eq!(classify_mailbox("Archive", "Archive"), MailboxRole::Archive);
        assert_eq!(classify_mailbox("Snoozed", "Snoozed"), MailboxRole::Snoozed);
    }

    #[test]
    fn system_folder_sort_order() {
        let mut folders = vec![
            folder("Trash", "Trash"),
            folder("Spam", "Spam"),
            folder("Archive", "Archive"),
            folder("Drafts", "Drafts"),
            folder("Sent", "Sent"),
            folder("Snoozed", "Snoozed"),
            folder("INBOX", "Inbox"),
        ];
        sort_folders(&mut folders);
        let paths: Vec<_> = folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["INBOX", "Snoozed", "Sent", "Drafts", "Archive", "Spam", "Trash"]
        );
    }

    #[test]
    fn label_tree_nested_and_unread_rollup() {
        let folders = vec![
            Folder {
                account_id: "a".into(),
                name: "работа".into(),
                path: "работа".into(),
                unread: 1,
                hidden: false,
            },
            Folder {
                account_id: "a".into(),
                name: "urgent".into(),
                path: "работа/urgent".into(),
                unread: 2,
                hidden: false,
            },
            Folder {
                account_id: "a".into(),
                name: "bookmarks".into(),
                path: "bookmarks".into(),
                unread: 0,
                hidden: false,
            },
        ];
        let tree = build_label_tree(&folders);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].segment, "bookmarks");
        assert_eq!(tree[0].display_name, "bookmarks");
        assert_eq!(tree[1].segment, "работа");
        assert_eq!(tree[1].display_name, "работа");
        assert_eq!(tree[1].unread, 1);
        assert_eq!(tree[1].unread_sum, 3);
        assert_eq!(tree[1].children.len(), 1);
        assert_eq!(tree[1].children[0].segment, "urgent");
        assert_eq!(tree[1].children[0].display_name, "urgent");
        assert_eq!(
            tree[1].children[0].full_path.as_deref(),
            Some("работа/urgent")
        );
    }

    #[test]
    fn label_tree_decodes_modified_utf7_display() {
        // Gmail-style wire path for «Работа».
        let folders = vec![Folder {
            account_id: "a".into(),
            name: "Работа".into(),
            path: "&BCAEMAQxBD4EQgQw-".into(),
            unread: 0,
            hidden: false,
        }];
        let tree = build_label_tree(&folders);
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].segment, "&BCAEMAQxBD4EQgQw-");
        assert_eq!(tree[0].display_name, "Работа");
    }
}
