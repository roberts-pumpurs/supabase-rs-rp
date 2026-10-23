use rp_supabase_codegen_example::database::public::tables::messages;

rp_supabase_client::projection! {
    struct Content for [
        rp_supabase_codegen_example::database::public::tables::articles,
        rp_supabase_codegen_example::database::public::tables::videos
    ] { name } filters { content_id: [id, id] }
}

fn main() {
    let _ = Content::content_id::<messages::Row>();
}
