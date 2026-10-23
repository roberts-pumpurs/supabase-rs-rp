use rp_supabase_client::select;
use rp_supabase_codegen_example::database::public::tables::{messages, orders};

fn main() {
    let _ = select!(messages::Row => { id, title });
    let _ = select!(orders::Row => { id, billing: orders_billing { lable } });
}
