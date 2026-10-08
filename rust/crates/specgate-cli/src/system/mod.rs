//! Operating-system filesystem and process boundaries used by CLI workflows.
//!
//! These services create scratch directories, execute discovery commands, and
//! preserve command environments across capture and replay operations.

mod discovery;
mod execution;
mod filesystem;

pub use discovery::Discovery;
pub(crate) use discovery::Failure as DiscoveryFailure;
pub use execution::Execution;
pub(crate) use execution::Request as ProcessRequest;
pub use filesystem::CommandEnvironment;
pub(crate) use filesystem::Scratch;

#[cfg(test)]
mod tests {
    use super::{CommandEnvironment, Execution, ProcessRequest};

    #[test]
    fn collision_retry() {
        let system = CommandEnvironment::fake();
        let collision = std::path::Path::new("fake-scratch").join("specgate-system-0");
        system.seed_directory(&collision);
        let scratch = system.scratch(None::<&std::path::Path>, "specgate-system-").unwrap();
        assert_eq!(scratch.path(), std::path::Path::new("fake-scratch").join("specgate-system-1"));
        assert!(system.is_directory(&collision));
        assert!(system.is_directory(scratch.path()));
    }

    #[test]
    fn guard_ownership() {
        let system = CommandEnvironment::fake();
        let first = system.scratch(None::<&std::path::Path>, "specgate-system-").unwrap();
        let first_path = first.path().to_path_buf();
        let second = system.scratch(None::<&std::path::Path>, "specgate-system-").unwrap();
        let second_path = second.path().to_path_buf();
        drop(first);
        assert!(!system.is_directory(&first_path));
        assert!(system.is_directory(&second_path));
        drop(second);
        assert!(!system.is_directory(&second_path));
    }

    #[test]
    fn allocation_failure() {
        let system = CommandEnvironment::fake();
        system.fail_next("scratch unavailable");
        assert_eq!(
            system
                .scratch(None::<&std::path::Path>, "specgate-system-")
                .unwrap_err()
                .to_string(),
            "scratch unavailable"
        );
    }

    #[test]
    fn process_behavior() {
        let execution = Execution::fake();
        execution.push(Err(std::io::Error::other("launch failed")));
        let request = ProcessRequest::builder("cargo").arg("check").build();
        assert_eq!(execution.run(&request).unwrap_err().to_string(), "launch failed");
        assert_eq!(execution.requests(), vec![request]);
    }
    #[test]
    fn fake_state() {
        let system = CommandEnvironment::fake();
        system.seed_file("input", b"bytes");
        system.seed_environment("TOOL", "fake-tool");
        assert_eq!(system.read("input").unwrap(), b"bytes");
        assert_eq!(system.environment("TOOL").as_deref(), Some(std::ffi::OsStr::new("fake-tool")));
        system.write("output", b"result").unwrap();
        assert!(system.is_file("output"));
        system.publish("published", b"atomic", ".tmp-").unwrap();
        assert_eq!(system.read("published").unwrap(), b"atomic");
        system.remove_file("output").unwrap();
        assert!(!system.is_file("output"));
    }
}
