use soroban_sdk::{testutils::Address as _, Address, Env, String};

use crate::contract::PrincipalTokenClient;
use crate::PrincipalToken;

/// A fresh PT with a generated admin, as the factory would deploy it.
pub fn register_pt(env: &Env) -> PrincipalTokenClient<'_> {
    let admin = Address::generate(env);
    let pt_addr = env.register(
        PrincipalToken,
        (
            &admin,
            String::from_str(env, "PT"),
            String::from_str(env, "PT"),
            7u32,
        ),
    );
    PrincipalTokenClient::new(env, &pt_addr)
}
