//! Project commands (`ghc project`).
//!
//! Manage GitHub Projects (v2) using the `ProjectV2` GraphQL API.

pub mod close;
pub mod copy;
pub mod create;
pub mod delete;
pub mod edit;
pub mod field_create;
pub mod field_delete;
pub mod field_list;
pub mod item_add;
pub mod item_archive;
pub mod item_create;
pub mod item_delete;
pub mod item_edit;
pub mod item_list;
pub mod link;
pub mod list;
pub mod mark_template;
pub mod unlink;
pub mod view;

use clap::Subcommand;

/// Manage projects.
#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Close a project.
    Close(close::CloseArgs),
    /// Copy a project.
    Copy(copy::CopyArgs),
    /// Create a project.
    Create(create::CreateArgs),
    /// Delete a project.
    Delete(delete::DeleteArgs),
    /// Edit a project.
    Edit(edit::EditArgs),
    /// Create a project field.
    #[command(name = "field-create")]
    FieldCreate(field_create::FieldCreateArgs),
    /// Delete a project field.
    #[command(name = "field-delete")]
    FieldDelete(field_delete::FieldDeleteArgs),
    /// List project fields.
    #[command(name = "field-list")]
    FieldList(field_list::FieldListArgs),
    /// Add an item to a project.
    #[command(name = "item-add")]
    ItemAdd(item_add::ItemAddArgs),
    /// Archive a project item.
    #[command(name = "item-archive")]
    ItemArchive(item_archive::ItemArchiveArgs),
    /// Create a draft issue in a project.
    #[command(name = "item-create")]
    ItemCreate(item_create::ItemCreateArgs),
    /// Delete a project item.
    #[command(name = "item-delete")]
    ItemDelete(item_delete::ItemDeleteArgs),
    /// Edit a project item.
    #[command(name = "item-edit")]
    ItemEdit(item_edit::ItemEditArgs),
    /// List project items.
    #[command(name = "item-list")]
    ItemList(item_list::ItemListArgs),
    /// Link a project to a repository.
    Link(link::LinkArgs),
    /// List projects.
    #[command(alias = "ls")]
    List(list::ListArgs),
    /// Mark a project as a template.
    #[command(name = "mark-template")]
    MarkTemplate(mark_template::MarkTemplateArgs),
    /// Unlink a project from a repository.
    Unlink(unlink::UnlinkArgs),
    /// View a project.
    View(view::ViewArgs),
}

impl ProjectCommand {
    /// Run the selected subcommand.
    ///
    /// # Errors
    ///
    /// Returns an error if the subcommand fails.
    pub async fn run(&self, factory: &crate::factory::Factory) -> anyhow::Result<()> {
        match self {
            Self::Close(args) => args.run(factory).await,
            Self::Copy(args) => args.run(factory).await,
            Self::Create(args) => args.run(factory).await,
            Self::Delete(args) => args.run(factory).await,
            Self::Edit(args) => args.run(factory).await,
            Self::FieldCreate(args) => args.run(factory).await,
            Self::FieldDelete(args) => args.run(factory).await,
            Self::FieldList(args) => args.run(factory).await,
            Self::ItemAdd(args) => args.run(factory).await,
            Self::ItemArchive(args) => args.run(factory).await,
            Self::ItemCreate(args) => args.run(factory).await,
            Self::ItemDelete(args) => args.run(factory).await,
            Self::ItemEdit(args) => args.run(factory).await,
            Self::ItemList(args) => args.run(factory).await,
            Self::Link(args) => args.run(factory).await,
            Self::List(args) => args.run(factory).await,
            Self::MarkTemplate(args) => args.run(factory).await,
            Self::Unlink(args) => args.run(factory).await,
            Self::View(args) => args.run(factory).await,
        }
    }
}
