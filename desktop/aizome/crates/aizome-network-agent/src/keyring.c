/* A small libsecret bridge. NetworkManager's established attributes let
 * Aizome read credentials saved by other desktops without migrating them.
 * This runs in a disposable worker process; the agent can cancel even an
 * outstanding keyring unlock prompt by terminating that worker. */
#include <libsecret/secret.h>

static const SecretSchema schema = {
    .name = "org.freedesktop.NetworkManager.Connection",
    .flags = SECRET_SCHEMA_DONT_MATCH_NAME,
    .attributes = {
        { "connection-uuid", SECRET_SCHEMA_ATTRIBUTE_STRING },
        { "setting-name", SECRET_SCHEMA_ATTRIBUTE_STRING },
        { "setting-key", SECRET_SCHEMA_ATTRIBUTE_STRING },
        { NULL, 0 }
    }
};

typedef void (*Emit)(void *, const char *, const char *);

/* "lookup" emits each matching key and secret; "list" emits each matching
 * item's setting and key, without reading secrets or unlocking. */
int aizome_keyring(const char *action, const char *uuid, const char *setting,
                   const char *key, const char *value, const char *label,
                   int unlock, Emit emit, void *context) {
    GError *error = NULL;
    int failed = 0;
    GHashTable *attrs = secret_attributes_build(&schema, "connection-uuid", uuid, NULL);
    if (*setting)
        g_hash_table_insert(attrs, g_strdup("setting-name"), g_strdup(setting));
    if (*key)
        g_hash_table_insert(attrs, g_strdup("setting-key"), g_strdup(key));
    if (g_str_equal(action, "lookup") || g_str_equal(action, "list")) {
        gboolean list = g_str_equal(action, "list");
        SecretSearchFlags flags = SECRET_SEARCH_ALL;
        if (!list)
            flags |= SECRET_SEARCH_LOAD_SECRETS;
        if (unlock && !list)
            flags |= SECRET_SEARCH_UNLOCK;
        GList *items = secret_service_search_sync(NULL, &schema, attrs, flags, NULL, &error);
        for (GList *it = items; it; it = it->next) {
            SecretItem *item = it->data;
            GHashTable *attributes = secret_item_get_attributes(item);
            const char *section = g_hash_table_lookup(attributes, "setting-name");
            const char *name = g_hash_table_lookup(attributes, "setting-key");
            if (list) {
                if (section && name)
                    emit(context, section, name);
            } else {
                SecretValue *secret = secret_item_get_secret(item);
                const char *text = secret ? secret_value_get_text(secret) : NULL;
                if (name && text)
                    emit(context, name, text);
                if (secret)
                    secret_value_unref(secret);
            }
            g_hash_table_unref(attributes);
        }
        g_list_free_full(items, g_object_unref);
    } else if (g_str_equal(action, "store")) {
        failed = !secret_password_storev_sync(&schema, attrs, SECRET_COLLECTION_DEFAULT,
                                              label, value, NULL, &error);
    } else if (g_str_equal(action, "delete")) {
        secret_password_clearv_sync(&schema, attrs, NULL, &error);
    }
    g_hash_table_unref(attrs);
    if (error) {
        /* Do not log credentials or connection data. */
        g_error_free(error);
        failed = 1;
    }
    return failed;
}
