// Mock with multiple responses — different inputs get different outputs.
use specgate::*;

#[spec_setup("get_users")]
pub fn make_service() -> UserService {
    UserService { db: RealDb {} }
}

pub struct RealDb {}
impl RealDb {
    pub fn find(&self, _id: &str) -> String {
        panic!("real db not available in test")
    }
}

pub struct UserService {
    pub db: RealDb,
}

impl UserService {
    #[spec_operation("get_users")]
    pub fn get_two_users(&self, id_a: &str, id_b: &str) -> String {
        #[spec_mock("db")]
        let a = self.db.find(id_a);
        #[spec_mock("db")]
        let b = self.db.find(id_b);
        format!("{a} and {b}")
    }
}
