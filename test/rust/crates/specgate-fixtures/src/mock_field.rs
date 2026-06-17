// Mock via call-site interception — on input X return Y.
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
    #[spec_operation("get_user")]
    fn get_user(&self, id: &str) -> String {
        #[spec_mock("db")]
        let response = self.db.find(id);
        response
    }
}
