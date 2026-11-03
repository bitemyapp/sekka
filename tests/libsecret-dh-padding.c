/* Force the GnuTLS boundary to return short secrets: a deterministic version
 * of the approximately 1/256 handshake that used to flake aizome-login.
 * The real libsecret backend must pad before feeding HKDF; no secret is logged.
 */
#include <glib.h>
#include <gnutls/abstract.h>
#include <string.h>
#include "egg/egg-dh.h"
#include "egg/egg-secure-memory.h"

EGG_SECURE_DEFINE_GLIB_GLOBALS();
static unsigned secret_length;

int __wrap_gnutls_privkey_derive_secret(gnutls_privkey_t privkey,
                                      gnutls_pubkey_t pubkey,
                                      const gnutls_datum_t *nonce,
                                      gnutls_datum_t *secret,
                                      unsigned flags)
{
    secret->size = secret_length;
    secret->data = gnutls_malloc(secret_length);
    memset(secret->data, 0x42, secret_length);
    return 0;
}

int main(void)
{
    egg_dh_params *params = egg_dh_default_params("ietf-ike-grp-modp-1024");
    egg_dh_pubkey *pub;
    egg_dh_privkey *priv;
    g_assert_true(egg_dh_gen_pair(params, 0, &pub, &priv));
    const unsigned lengths[] = {1, 2, 127, 128};
    for (unsigned i = 0; i < G_N_ELEMENTS(lengths); ++i) {
        secret_length = lengths[i];
        GBytes *bytes = egg_dh_gen_secret(pub, priv, params);
        g_assert_nonnull(bytes);
        gsize length;
        const unsigned char *data = g_bytes_get_data(bytes, &length);
        if (length != 128) {
            g_printerr("DH_PADDING_MISSING: got %zu bytes, expected 128\n", length);
            return 1;
        }
        for (unsigned j = 0; j < length; ++j)
            g_assert_cmpuint(data[j], ==, j < 128 - secret_length ? 0 : 0x42);
        g_bytes_unref(bytes);
    }
    egg_dh_pubkey_free(pub);
    egg_dh_privkey_free(priv);
    egg_dh_params_free(params);
    return 0;
}
