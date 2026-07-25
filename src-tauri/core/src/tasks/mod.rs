//! Task documents: `type: Task` markdown files under a vault's tasks folder.
//! Pure filename/render/parse logic + the two sanctioned vault writes
//! (collision-safe create; surgical `status:` flip). Same never-clobber
//! discipline as the capture note and transcript sidecar. See
//! docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md.

mod collect;
mod create;
mod description;
mod disk;
mod doc;
mod hierarchy;
mod id;
mod list;
mod lists;
mod parent;
mod parent_link;
mod parse;
mod structural;
mod writer;

/// The reserved structured-task frontmatter keys — the fields the surgical
/// reader/writer own. ONE source of truth for the two guards that must agree
/// on this set (a divergence reopens the GAP-68/GAP-77 class): the
/// template-frontmatter filter (`create::render_task` drops any of these a
/// user template tries to seed, so `set_fields` is never confused about which
/// key it owns) and the task-ID-property validator (`id::is_valid_id_property`
/// refuses one as the ID property, so the ID writer can't clobber a real
/// field). `description` is included: it is a MANAGED detail-view field like
/// `due`/`status`, so a template seeding it — a block scalar especially —
/// would orphan its content on the first surgical save (Codex PR #76). Held
/// in the parent module (not duplicated per guard) so the two can never drift
/// — the compiler enforces what a `// keep in sync` comment only asked for
/// (GAP-70).
const RESERVED_TASK_KEYS: &[&str] = &[
    "type",
    "status",
    "title",
    "created",
    "due",
    "scheduled",
    "priority",
    "tags",
    "tag",
    "order",
    "description",
    "parent-id",
    "parent",
];

pub use create::{create_task, render_task, task_basename};
pub use disk::{backfill_task_id, set_task_status, update_task_fields};
pub use doc::is_task;
pub use hierarchy::{
    ambiguous_ids, ancestors, parent_index, parent_index_for_validation, would_create_cycle,
    ParentIndex,
};
pub use id::{id_property_for_generation, is_valid_id_property, new_task_id};
pub use list::{list_tasks, list_tasks_structural, priority_rank, TaskItem};
pub use lists::{
    create_task_list, delete_task_list, is_valid_list_name, move_task_to_list, normalize_list_rel,
    rename_task_list, task_lists, DeleteListOutcome,
};
pub use parent_link::compose as compose_parent_link;
pub use parse::{is_valid_due, is_valid_tag, note_tags};
pub use structural::{delete_task, duplicate_task};
pub use writer::{set_fields, set_status};
