-- Run in a disposable database as its owner before starting PostgREST.
-- PostgREST must expose public with a role granted access below; no secrets are stored here.
CREATE TYPE public.mood AS ENUM ('happy', 'needs-review');
CREATE TABLE public.messages (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    body text NOT NULL,
    note text,
    created_at timestamptz NOT NULL DEFAULT now(),
    mood public.mood NOT NULL DEFAULT 'happy',
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,
    amount numeric NOT NULL DEFAULT 0,
    tags text[] NOT NULL DEFAULT '{}'::text[]
);
CREATE VIEW public.message_summaries AS SELECT id, body FROM public.messages;
CREATE FUNCTION public.echo_message(message text) RETURNS text
LANGUAGE sql IMMUTABLE AS $$ SELECT message $$;
-- Single unnamed row-type arguments make computed fields, not RPCs.
CREATE FUNCTION public.excerpt(public.messages) RETURNS text
LANGUAGE sql STABLE AS $$ SELECT left($1.body, 12) $$;
CREATE FUNCTION public.message_meta(public.messages) RETURNS jsonb
LANGUAGE sql STABLE AS $$ SELECT jsonb_build_object('id', $1.id, 'tags', $1.tags) $$;
CREATE TABLE public.countries (
    id bigint PRIMARY KEY,
    name text NOT NULL
);
CREATE TABLE public.addresses (
    id bigint PRIMARY KEY,
    label text NOT NULL,
    country_id bigint,
    CONSTRAINT address_country FOREIGN KEY (country_id) REFERENCES public.countries(id)
);
CREATE TABLE public.customers (
    id bigint PRIMARY KEY,
    name text NOT NULL
);
CREATE TABLE public.orders (
    id bigint PRIMARY KEY,
    customer_id bigint NOT NULL,
    billing_id bigint,
    shipping_id bigint,
    label text NOT NULL,
    CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES public.customers(id),
    CONSTRAINT orders_billing FOREIGN KEY (billing_id) REFERENCES public.addresses(id),
    CONSTRAINT orders_shipping FOREIGN KEY (shipping_id) REFERENCES public.addresses(id)
);
CREATE TABLE public.order_details (
    order_id bigint PRIMARY KEY,
    note text NOT NULL,
    CONSTRAINT details_order FOREIGN KEY (order_id) REFERENCES public.orders(id)
);
-- A UNIQUE foreign key is also a reverse to-one, without being the primary key.
CREATE TABLE public.customer_preferences (
    id bigint PRIMARY KEY,
    customer_id bigint NOT NULL UNIQUE,
    label text NOT NULL,
    CONSTRAINT preferences_customer FOREIGN KEY (customer_id) REFERENCES public.customers(id)
);
CREATE TABLE public.composite_parents (
    tenant_id bigint NOT NULL,
    id bigint NOT NULL,
    label text NOT NULL,
    PRIMARY KEY (tenant_id, id)
);
CREATE TABLE public.composite_children (
    id bigint PRIMARY KEY,
    parent_id bigint NOT NULL,
    tenant_id bigint NOT NULL,
    label text NOT NULL,
    -- Deliberately pair columns in an order different from the target PK.
    CONSTRAINT composite_parent FOREIGN KEY (parent_id, tenant_id)
        REFERENCES public.composite_parents(id, tenant_id)
);
CREATE TABLE public.articles (
    id bigint PRIMARY KEY, name text NOT NULL, owner_id bigint NOT NULL, document jsonb NOT NULL
);
CREATE TABLE public.videos (
    id bigint PRIMARY KEY, name text NOT NULL, owner_id bigint NOT NULL, document jsonb NOT NULL
);
CREATE TABLE public.content_wrong_type (
    id bigint PRIMARY KEY, name text NOT NULL, owner_id text NOT NULL, document jsonb NOT NULL
);
CREATE TABLE public.content_wrong_key (
    id bigint PRIMARY KEY, name text NOT NULL, "owner.id" bigint NOT NULL, document jsonb NOT NULL
);
CREATE TABLE public.content_missing (
    id bigint PRIMARY KEY, name text NOT NULL, document jsonb NOT NULL
);
CREATE TABLE public.content_links (
    id bigint PRIMARY KEY, article_id bigint NOT NULL, video_id bigint NOT NULL,
    CONSTRAINT links_article FOREIGN KEY (article_id) REFERENCES public.articles(id),
    CONSTRAINT links_video FOREIGN KEY (video_id) REFERENCES public.videos(id)
);
GRANT SELECT, INSERT, UPDATE, DELETE ON public.content_links TO PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON public.articles, public.videos,
    public.content_wrong_type, public.content_wrong_key, public.content_missing TO PUBLIC;
-- These grants are for a disposable smoke database only, not a production policy.
GRANT USAGE ON SCHEMA public TO PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON public.messages TO PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON public.countries, public.addresses,
    public.customers, public.orders, public.order_details, public.customer_preferences,
    public.composite_parents, public.composite_children TO PUBLIC;
GRANT SELECT ON public.message_summaries TO PUBLIC;
GRANT USAGE, SELECT ON SEQUENCE public.messages_id_seq TO PUBLIC;
GRANT EXECUTE ON FUNCTION public.echo_message(text) TO PUBLIC;
NOTIFY pgrst, 'reload schema';
