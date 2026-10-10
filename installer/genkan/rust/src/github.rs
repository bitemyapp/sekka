// SPDX-License-Identifier: GPL-3.0-or-later
//! GitHub onboarding, as Ubuntu's server installer offers it: a GitHub
//! account's public SSH keys for the installed user's authorized keys, the
//! OpenSSH server, and a Git identity for commits.
//!
//! The GUI fetches keys and profile over HTTPS. Like every other field they
//! reach the privileged helper as untrusted request data and are parsed again
//! there; only parsed values are written to configuration.nix.
use anyhow::{Context, Result, bail, ensure};
use base64ct::{Base64, Base64Unpadded, Encoding};
use sha2::{Digest, Sha256};

/// A GitHub login: 1–39 letters, digits or single hyphens, not at either end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GithubUser(String);
impl GithubUser {
    pub fn parse(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        ensure!(
            !bytes.is_empty()
                && bytes.len() <= 39
                && bytes
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
                && bytes[0] != b'-'
                && bytes[bytes.len() - 1] != b'-'
                && !value.contains("--"),
            "A GitHub username has up to 39 letters, digits or single hyphens, not at either end"
        );
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Key types OpenSSH accepts for user authentication, with a short name.
const ALGORITHMS: [(&str, &str); 7] = [
    ("ssh-ed25519", "ED25519"),
    ("sk-ssh-ed25519@openssh.com", "ED25519-SK"),
    ("ecdsa-sha2-nistp256", "ECDSA"),
    ("ecdsa-sha2-nistp384", "ECDSA"),
    ("ecdsa-sha2-nistp521", "ECDSA"),
    ("sk-ecdsa-sha2-nistp256@openssh.com", "ECDSA-SK"),
    ("ssh-rsa", "RSA"),
];
/// Larger than a 16384-bit RSA key.
const MAX_BLOB: usize = 4096;
/// Authorized keys from one account.
pub const MAX_KEYS: usize = 64;

/// One public key as authorized_keys lists it: algorithm and base64 key,
/// with no options and no comment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizedKey {
    algorithm: &'static str,
    kind: &'static str,
    blob: Vec<u8>,
}
impl AuthorizedKey {
    /// `algorithm base64 [comment]`; a comment is dropped. The key must name
    /// the same algorithm inside as outside, as OpenSSH checks.
    pub fn parse(line: &str) -> Result<Self> {
        ensure!(
            line.len() <= 2 * MAX_BLOB && !line.chars().any(char::is_control),
            "An SSH public key must be one line"
        );
        let mut fields = line.split_ascii_whitespace();
        let (Some(name), Some(encoded)) = (fields.next(), fields.next()) else {
            bail!("An SSH public key needs an algorithm and the key");
        };
        let Some((algorithm, kind)) = ALGORITHMS.iter().copied().find(|(a, _)| *a == name) else {
            bail!("Unsupported SSH key type");
        };
        let blob = Base64::decode_vec(encoded).map_err(|_| anyhow::anyhow!("Invalid SSH key"))?;
        ensure!(blob.len() <= MAX_BLOB, "SSH key is too large");
        let declared = blob
            .get(..4)
            .map(|n| u32::from_be_bytes([n[0], n[1], n[2], n[3]]) as usize)
            .and_then(|length| blob.get(4..4 + length))
            .context("Invalid SSH key")?;
        ensure!(
            declared == algorithm.as_bytes() && blob.len() > 4 + declared.len(),
            "The SSH key's type does not match its contents"
        );
        Ok(Self {
            algorithm,
            kind,
            blob,
        })
    }
    /// The authorized_keys line: algorithm and base64 key.
    pub fn text(&self) -> String {
        format!("{} {}", self.algorithm, Base64::encode_string(&self.blob))
    }
    /// ED25519, ECDSA, RSA, or one of the security-key (-SK) types.
    pub fn kind(&self) -> &'static str {
        self.kind
    }
    /// As `ssh-keygen -l` prints it: SHA256: and unpadded base64.
    pub fn fingerprint(&self) -> String {
        format!(
            "SHA256:{}",
            Base64Unpadded::encode_string(&Sha256::digest(&self.blob))
        )
    }
}

/// Keys from an account's https://github.com/USER.keys listing, one per
/// line. Lines of an unsupported type are skipped and counted.
pub fn parse_listing(text: &str) -> Result<(Vec<AuthorizedKey>, usize)> {
    let mut keys = Vec::new();
    let mut skipped = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match AuthorizedKey::parse(line) {
            Ok(key) if !keys.contains(&key) => keys.push(key),
            Ok(_) => {}
            Err(_) => skipped += 1,
        }
    }
    ensure!(keys.len() <= MAX_KEYS, "More than {MAX_KEYS} SSH keys");
    Ok((keys, skipped))
}

/// The name and address Git records in commits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitIdentity {
    name: String,
    email: String,
}
impl GitIdentity {
    /// Both, or neither for no identity.
    pub fn parse(name: &str, email: &str) -> Result<Option<Self>> {
        let (name, email) = (name.trim(), email.trim());
        if name.is_empty() && email.is_empty() {
            return Ok(None);
        }
        ensure!(
            !name.is_empty() && !email.is_empty(),
            "Give Git both a name and an email address, or neither"
        );
        ensure!(
            name.len() <= 128 && !name.chars().any(|c| c.is_control() || "<>".contains(c)),
            "The Git name cannot contain < > or control characters, and is limited to 128 bytes"
        );
        let address = email
            .split_once('@')
            .filter(|(local, domain)| !local.is_empty() && !domain.is_empty());
        ensure!(
            email.len() <= 254
                && address.is_some()
                && !email
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || "<>".contains(c)),
            "Enter the email address Git should record, such as you@example.com"
        );
        Ok(Some(Self {
            name: name.into(),
            email: email.into(),
        }))
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn email(&self) -> &str {
        &self.email
    }
}

/// Everything the GitHub step configures. Keys always come from an account.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Onboarding {
    github: Option<GithubUser>,
    keys: Vec<AuthorizedKey>,
    ssh_server: bool,
    git: Option<GitIdentity>,
}
impl Onboarding {
    pub fn parse(
        github: &str,
        keys: &[String],
        ssh_server: bool,
        git_name: &str,
        git_email: &str,
    ) -> Result<Self> {
        let github = (!github.is_empty())
            .then(|| GithubUser::parse(github))
            .transpose()?;
        ensure!(
            keys.is_empty() || github.is_some(),
            "Authorized SSH keys must come from a GitHub account"
        );
        ensure!(keys.len() <= MAX_KEYS, "More than {MAX_KEYS} SSH keys");
        let mut parsed: Vec<AuthorizedKey> = Vec::new();
        for key in keys {
            let key = AuthorizedKey::parse(key)?;
            ensure!(!parsed.contains(&key), "Duplicate SSH key");
            parsed.push(key);
        }
        Ok(Self {
            github,
            keys: parsed,
            ssh_server,
            git: GitIdentity::parse(git_name, git_email)?,
        })
    }
    pub fn github(&self) -> Option<&GithubUser> {
        self.github.as_ref()
    }
    pub fn keys(&self) -> &[AuthorizedKey] {
        &self.keys
    }
    pub fn ssh_server(&self) -> bool {
        self.ssh_server
    }
    /// With keys authorized, the server refuses passwords.
    pub fn password_login(&self) -> bool {
        self.ssh_server && self.keys.is_empty()
    }
    pub fn git(&self) -> Option<&GitIdentity> {
        self.git.as_ref()
    }
    /// The request fields: GitHub user, keys, server, Git name and email.
    pub fn into_raw(self) -> (String, Vec<String>, bool, String, String) {
        let (name, email) = self
            .git
            .map_or_else(Default::default, |g| (g.name, g.email));
        (
            self.github.map(|u| u.0).unwrap_or_default(),
            self.keys.iter().map(AuthorizedKey::text).collect(),
            self.ssh_server,
            name,
            email,
        )
    }
}

/// What a GitHub account offers the installer.
#[derive(Clone, Debug)]
pub struct Profile {
    /// The account's own spelling of its login.
    pub user: GithubUser,
    pub keys: Vec<AuthorizedKey>,
    /// Keys of a type OpenSSH does not accept here.
    pub skipped: usize,
    /// The profile's display name, if it has one.
    pub name: Option<String>,
    /// The profile's public address, or GitHub's private commit address.
    pub email: Option<String>,
    pub private_email: bool,
}

/// GitHub's private commit address for an account.
pub fn noreply_email(id: u64, user: &GithubUser) -> String {
    format!("{id}+{}@users.noreply.github.com", user.as_str())
}

/// An HTTPS GET of at most `limit` bytes: the status code and the body.
fn get(url: &str, limit: usize, headers: &[&str]) -> Result<(u16, String)> {
    let limit = limit.to_string();
    let mut args = vec![
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--connect-timeout",
        "5",
        "--max-time",
        "10",
        "--max-filesize",
        &limit,
        "--write-out",
        "\n%{http_code}",
    ];
    for header in headers {
        args.extend(["--header", header]);
    }
    if let Some(ca) = option_env!("CALAMARES_CA_FILE") {
        args.extend(["--cacert", ca]);
    }
    args.extend(["--url", url]);
    let output = crate::process::output_full("curl", &args, 15)
        .map_err(|_| anyhow::anyhow!("Could not reach GitHub. Check the network connection."))?;
    let (body, status) = output.rsplit_once('\n').context("No answer from GitHub")?;
    Ok((
        status.trim().parse().context("No answer from GitHub")?,
        body.into(),
    ))
}

/// Look an account up: its public keys (required) and its profile (best
/// effort; the API allows 60 unauthenticated requests an hour). Run on a
/// worker: this can take several seconds.
pub fn fetch(user: &GithubUser) -> Result<Profile> {
    let (status, listing) = get(
        &format!("https://github.com/{}.keys", user.as_str()),
        64 * 1024,
        &[],
    )?;
    match status {
        200 => {}
        404 => bail!("There is no GitHub account named {}.", user.as_str()),
        status => bail!("GitHub answered with HTTP {status}. Try again later."),
    }
    let (keys, skipped) = parse_listing(&listing)?;
    let profile = get(
        &format!("https://api.github.com/users/{}", user.as_str()),
        64 * 1024,
        &[
            "Accept: application/vnd.github+json",
            "X-GitHub-Api-Version: 2022-11-28",
        ],
    )
    .ok()
    .filter(|(status, _)| *status == 200)
    .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(&body).ok());
    let login = profile
        .as_ref()
        .and_then(|p| p["login"].as_str())
        .and_then(|login| GithubUser::parse(login).ok())
        .filter(|login| login.as_str().eq_ignore_ascii_case(user.as_str()))
        .unwrap_or_else(|| user.clone());
    let text = |field: &str| {
        profile
            .as_ref()
            .and_then(|p| p[field].as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    };
    let public = text("email").filter(|e| GitIdentity::parse("x", e).is_ok());
    let private = profile
        .as_ref()
        .and_then(|p| p["id"].as_u64())
        .map(|id| noreply_email(id, &login));
    Ok(Profile {
        name: text("name").filter(|n| GitIdentity::parse(n, "x@y").is_ok()),
        private_email: public.is_none() && private.is_some(),
        email: public.or(private),
        user: login,
        keys,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Throwaway keys from ssh-keygen; fingerprints from `ssh-keygen -l`.
    const ED25519: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIF8y4b2kcB5eEPbrb5tU80+lphiIk3o7v09PPFOqRtH3";
    const ED25519_FINGERPRINT: &str = "SHA256:4zQWvJbDI9/1zc3zovpAabT/Ij3+wOS0kGsoAmuZv50";
    const ECDSA: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBFaKW8Z6YOoa5UaaVHwOgDXPe4PcYk663+2rvgwaCVo0QOYEMFJWQ4VZL6GKzJuSioxTotYTGYsKGOv45zDqz0A=";
    const RSA: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQCndhc2oCBfR/iu5iAEeXXNmUQu8vVZzO9vNhKQgRADXwHgsiKQ5WsIjGHjR0Erw14jbGgNsMQ+01Qtn/kp3bSFtknue+6yHCVJvhlbKvgPCAZ4HihZDKtgyKV71YUdsYu6BkgFtlyIyqq043yGyaOcME2/a5g6NftkPOtIlTmb5hKPXzRBcQ6sTqJ2EloKefy7hzQbOflrhDezmrqkkoGl0qsyQIhPYPz7zuuFKwSPnDdPfdH2GOzLV3m9/A5vOiVJ+E94hS9Fh0sMIM8ljEWgZDqtbFBOAqD99/JTrbulTV/505ftuNkeKGKoWIljoKezsaaHtyVAG9lNDklb/7NyneaNVA1v65r649KTwTE291l0gCReq+sgqpASMw8pzNSt/adO6q3MnsaRmVcOc5ngt2mDt4ptN815qTD4z0b6rl1wlprg6Fbv8SdkootQa24m07+TyiMiTO2NQvQJRBaUqu/RKUUVXiYP2ySl84EuuG1/XE2CUAaAovIn+DVIKHc=";

    #[test]
    fn github_usernames() {
        for good in ["a", "bitemyapp", "Octo-Cat", "a1-b2", &"x".repeat(39)] {
            assert!(GithubUser::parse(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "a_b",
            "a b",
            "a.b",
            &"x".repeat(40),
            "ü",
        ] {
            assert!(GithubUser::parse(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn keys_keep_only_algorithm_and_key() {
        let key = AuthorizedKey::parse(&format!("{ED25519} alice@laptop")).unwrap();
        assert_eq!(key.text(), ED25519);
        assert_eq!(key.kind(), "ED25519");
        assert_eq!(key.fingerprint(), ED25519_FINGERPRINT);
        for (line, kind, fingerprint) in [
            (
                ECDSA,
                "ECDSA",
                "SHA256:3QEZRqfHMfkdRQGLoZKpFi62b+SRFqtttM9pUgQl08U",
            ),
            (
                RSA,
                "RSA",
                "SHA256:b8rw65XcwpU89mVjRJoLXERj3RsRnEMFAK+A98t28Ag",
            ),
        ] {
            let key = AuthorizedKey::parse(line).unwrap();
            assert_eq!((key.text().as_str(), key.kind()), (line, kind));
            assert_eq!(key.fingerprint(), fingerprint);
        }
    }
    #[test]
    fn keys_are_checked_against_their_contents() {
        let (_, encoded) = ED25519.split_once(' ').unwrap();
        for bad in [
            format!("ssh-rsa {encoded}"),
            format!("ssh-dss {encoded}"),
            format!("ssh-ed25519 {}", &encoded[..20]),
            "ssh-ed25519 not*base64".into(),
            "ssh-ed25519".into(),
            format!("command=\"sh\" {ED25519}"),
            format!("{ED25519}\nssh-ed25519 {encoded}"),
            format!(
                "ssh-ed25519 {}",
                Base64::encode_string(b"\0\0\0\x0bssh-ed25519")
            ),
        ] {
            assert!(AuthorizedKey::parse(&bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn listings_skip_unsupported_keys_and_duplicates() {
        let listing = format!("{ED25519}\n\nssh-dss AAAAB3NzaC1kc3M=\n{ED25519}\n");
        let (keys, skipped) = parse_listing(&listing).unwrap();
        assert_eq!((keys.len(), skipped), (1, 1));
        assert_eq!(parse_listing("").unwrap().0.len(), 0);
    }
    #[test]
    fn git_identity_is_both_or_neither() {
        assert_eq!(GitIdentity::parse(" ", "").unwrap(), None);
        let id = GitIdentity::parse(" Ada Lovelace ", "ada@example.com")
            .unwrap()
            .unwrap();
        assert_eq!((id.name(), id.email()), ("Ada Lovelace", "ada@example.com"));
        for (name, email) in [
            ("Ada", ""),
            ("", "ada@example.com"),
            ("Ada <x>", "ada@example.com"),
            ("Ada\nLovelace", "ada@example.com"),
            ("Ada", "ada"),
            ("Ada", "@example.com"),
            ("Ada", "ada@"),
            ("Ada", "a da@example.com"),
            ("Ada", "<ada@example.com>"),
        ] {
            assert!(
                GitIdentity::parse(name, email).is_err(),
                "{name:?} {email:?}"
            );
        }
    }
    #[test]
    fn onboarding_requires_an_account_for_keys() {
        let keys = vec![ED25519.to_owned()];
        assert!(Onboarding::parse("", &keys, true, "", "").is_err());
        assert!(Onboarding::parse("ada", &[ED25519.into(), ED25519.into()], true, "", "").is_err());
        let chosen = Onboarding::parse("ada", &keys, true, "Ada", "ada@example.com").unwrap();
        assert!(chosen.ssh_server() && !chosen.password_login());
        assert_eq!(
            chosen.clone().into_raw(),
            (
                "ada".into(),
                keys,
                true,
                "Ada".into(),
                "ada@example.com".into()
            )
        );
        let server_only = Onboarding::parse("", &[], true, "", "").unwrap();
        assert!(server_only.password_login() && server_only.github().is_none());
        assert_eq!(
            Onboarding::parse("", &[], false, "", "").unwrap(),
            Onboarding::default()
        );
    }
    #[test]
    fn private_commit_address() {
        let user = GithubUser::parse("octocat").unwrap();
        assert_eq!(
            noreply_email(583231, &user),
            "583231+octocat@users.noreply.github.com"
        );
    }
}
