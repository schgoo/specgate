// Mock via call-site interception — on input X return Y.
use specgate_annotations::*;

#[spec_setup("make_service")]
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
    #[spec_operation("get_user")]
    pub fn get_user(&self, id: &str) -> String {
        #[spec_mock("db")]
        let response = self.db.find(id);
        response
    }
}
