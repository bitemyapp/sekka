"""Compile the real injection/conversion functions with a minimal mac80211 fixture.

This checks firmware rate encoding, not radio operation. The unpatched 7.3.3
source fails the first 5-GHz assertion; the complete module is built separately.
"""
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
tx = (root / "mld/tx.c").read_text()
rates = (root / "fw/api/rs.h").read_text()


def function(name):
    start = tx.index("static u32 " + name + "(")
    end = tx.index("\n}\n", start) + 3
    return tx[start:end]


# Use the driver's actual rate indices and firmware bit definitions.
enum_start = rates.index("enum {\n\tIWL_RATE_1M_INDEX")
enum_end = rates.index("\n};", enum_start) + 3
defines = re.findall(r"^#define RATE_[^\n]*", rates.replace("\\\n", ""), re.M)
source = r"""
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
typedef uint8_t u8;
typedef uint16_t __le16;
typedef uint32_t u32;
#define BIT(n) (1U << (n))
#define WARN_ON_ONCE(x) assert(!(x))
#define NL80211_BAND_2GHZ 0
#define IEEE80211_TX_RC_VHT_MCS BIT(0)
#define IEEE80211_TX_RC_MCS BIT(1)
#define IEEE80211_TX_RC_SHORT_GI BIT(2)
#define IEEE80211_TX_RC_40_MHZ_WIDTH BIT(3)
#define IEEE80211_TX_RC_80_MHZ_WIDTH BIT(4)
#define IEEE80211_TX_RC_160_MHZ_WIDTH BIT(5)
#define IEEE80211_TX_CTL_LDPC BIT(6)
#define IEEE80211_TX_CTL_STBC BIT(7)
struct iwl_mld { int unused; };
struct ieee80211_sta { int unused; };
struct ieee80211_tx_rate { int idx; u32 flags; };
struct ieee80211_tx_info {
    u32 band, flags;
    struct {
        struct ieee80211_tx_rate rates[1];
        u32 antennas;
        void *vif;
    } control;
};
static u32 u32_encode_bits(u32 v, u32 mask) {
    return (v << __builtin_ctz(mask)) & mask;
}
static u32 u32_get_bits(u32 v, u32 mask) {
    return (v & mask) >> __builtin_ctz(mask);
}
static u8 ieee80211_rate_get_vht_mcs(struct ieee80211_tx_rate *r) {
    return r->idx & 15;
}
static u8 ieee80211_rate_get_vht_nss(struct ieee80211_tx_rate *r) {
    return (r->idx >> 4) + 1;
}
static u8 iwl_mld_get_lowest_rate(struct iwl_mld *m,
                                struct ieee80211_tx_info *i, void *v) {
    return 4; /* Fixture's lowest basic rate is 6 Mbps OFDM. */
}
static u32 iwl_mld_get_tx_ant(struct iwl_mld *m, struct ieee80211_tx_info *i,
                            struct ieee80211_sta *s, __le16 fc) { return 0; }
"""
source += rates[enum_start:enum_end] + "\n" + "\n".join(defines) + "\n"
source += function("iwl_mld_mac80211_rate_idx_to_fw")
source += function("iwl_mld_get_inject_tx_rate")
source += r"""
int main(void) {
    struct ieee80211_tx_info info = {0};
    /* All twelve 2.4-GHz rates and all eight 5/6-GHz OFDM rates. */
    for (int band = 0; band < 3; ++band) {
        info.band = band;
        int count = band ? 8 : 12;
        for (int index = 0; index < count; ++index) {
            info.control.rates[0].idx = index;
            u32 actual = iwl_mld_get_inject_tx_rate(NULL, &info, NULL, 0);
            u32 expected = band ? (0x100U | index) :
                           index < 4 ? index : (0x100U | (index - 4));
            if (actual != expected) {
                fprintf(stderr, "band %d index %d: got %#x, expected %#x\n",
                        band, index, actual, expected);
                return 1;
            }
        }
        info.control.rates[0].idx = -1;
        assert(iwl_mld_get_inject_tx_rate(NULL, &info, NULL, 0) == 0x100);
    }
    /* HT MCS7 must retain its encoding when the band changes. */
    info.control.rates[0].flags = IEEE80211_TX_RC_MCS;
    info.control.rates[0].idx = 7;
    for (int band = 0; band < 3; ++band) {
        info.band = band;
        assert(iwl_mld_get_inject_tx_rate(NULL, &info, NULL, 0) == 0x207);
    }
    puts("Intel injection: 28 legacy mappings, 3 fallbacks and 3 HT rates passed");
}
"""
with tempfile.TemporaryDirectory() as temp:
    test = Path(temp) / "rates.c"
    binary = Path(temp) / "rates"
    test.write_text(source)
    subprocess.run([os.environ.get("CC", "cc"), "-std=gnu11", "-Wall", "-Werror",
                    str(test), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
