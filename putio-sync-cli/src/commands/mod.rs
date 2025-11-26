//! Command implementations for putio-sync CLI

pub mod create;
pub mod daemon;
pub mod delete;
pub mod download;
pub mod history;
pub mod list;
pub mod login;
pub mod queue;
pub mod server;
pub mod stat;
pub mod status;
pub mod transfers;

pub use create::execute_create;
pub use daemon::execute_daemon;
pub use delete::execute_delete;
pub use download::execute_download;
pub use history::execute_history;
pub use list::execute_list;
pub use login::execute_login;
pub use server::execute_server;
pub use stat::execute_stat;
pub use status::{execute_debug, execute_health, execute_status};
