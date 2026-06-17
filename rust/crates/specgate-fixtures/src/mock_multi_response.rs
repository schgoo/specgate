// Mock with multiple responses — different inputs get different outputs.
use specgate_annotations::*;

#[spec_setup("make_service")]
fn make_service() -> UserService {
    UserService { db: RealDb {} }
}

struct RealDb {}
impl RealDb {
    fn find(&self, id: &str) -> String {
        panic!("real db not available in test")
    }
}

struct UserService {
    db: RealDb,
}

impl UserService {
    #[spec_operation("get_users")]
    fn get_two_users(&self, id_a: &str, id_b: &str) -> String {
        #[spec_mock("db")]
        let a = self.db.find(id_a);
        #[spec_mock("db")]
        let b = self.db.find(id_b);
        format!("{a} and {b}")
    }
}
