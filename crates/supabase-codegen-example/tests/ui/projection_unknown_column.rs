use rp_supabase_client::projection;
use rp_supabase_codegen_example::database::public::tables::orders;

projection! {
    struct Invalid for orders { id, titl }
}

fn main() {}
