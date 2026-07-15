//! Task documents: `type: Task` markdown files under a vault's tasks folder.
//! Pure filename/render/parse logic + the two sanctioned vault writes
//! (collision-safe create; surgical `status:` flip). Same never-clobber
//! discipline as the capture note and transcript sidecar. See
//! docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md.

mod disk;
mod doc;
mod id;
mod list;
mod lists;
mod parse;
mod writer;

pub use disk::{create_task, render_task, set_task_status, task_basename, update_task_fields};
pub use doc::is_task;
pub use id::{is_valid_id_property, new_task_id};
pub use list::{list_tasks, priority_rank, TaskItem};
pub use lists::{
    create_task_list, is_valid_list_name, move_task_to_list, normalize_list_rel, task_lists,
};
pub use parse::{is_valid_due, is_valid_tag, note_tags};
pub use writer::{set_fields, set_status};
