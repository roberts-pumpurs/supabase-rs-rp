use rp_supabase_client::{key, select};
use rp_supabase_client::schema::Selection;
use rp_supabase_codegen_example::database::public::tables::orders;

fn main() {
    let selected = select!(orders::Row => { id, billing: orders_billing { id } });
    let _ = selected.column(key!(titl));
    let _ = selected.billing.column(key!(lable));
}
