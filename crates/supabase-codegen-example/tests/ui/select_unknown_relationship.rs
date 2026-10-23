use rp_supabase_client::select;
use rp_supabase_codegen_example::database::public::tables::orders;

fn main() {
    let _ = select!(orders::Row => { id, billing: orders_bling { label } });
}
