//! Release-lag regression tests through the public CLI and local repositories.
mod support;

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::UNIX_EPOCH;
use support::*;

struct Repository {
    root: TempPath,
    address: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    fault: Arc<Mutex<Option<(String, u16)>>>,
    server: Option<thread::JoinHandle<()>>,
}

impl Repository {
    fn new() -> Self {
        let root = temp_dir("ir-binary-repository");
        let out = Command::new(rscript())
            .arg("--vanilla")
            .arg(fixture("binary-repository.R"))
            .arg(&root)
            .output()
            .unwrap();
        assert_success(&out);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let directory = root.to_path_buf();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requests);
        let fault: Arc<Mutex<Option<(String, u16)>>> = Arc::new(Mutex::new(None));
        let server_fault = Arc::clone(&fault);
        let server = thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while stream.read(&mut byte).unwrap_or(0) == 1 {
                    request.push(byte[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8(request).unwrap();
                let mut parts = request.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("/");
                if path == "/stop" {
                    break;
                }
                log.lock().unwrap().push(path.to_owned());
                let path = path.split('?').next().unwrap();
                let content = fs::read(directory.join(path.trim_start_matches('/')));
                let (mut status, mut body) = match content {
                    Ok(body) => (200, body),
                    Err(_) => (404, Vec::new()),
                };
                if let Some((pattern, code)) = &*server_fault.lock().unwrap() {
                    if path.contains(pattern) {
                        status = *code;
                        body.clear();
                    }
                }
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                if method != "HEAD" {
                    let _ = stream.write_all(&body);
                }
            }
        });
        fs::write(root.join("profile.R"), format!(
            "options(repos = c(CRAN = 'http://{address}/repo'), pkg.extra_arch_repos = TRUE, pkg.use_bioconductor = FALSE, pkg.cran_metadata_url = 'http://{address}/metadata/', pak.no_extra_messages = TRUE)\nif (nzchar(Sys.getenv('IR_RESOLVE_RESULT_FILE'))) cat('resolve\\n', file = Sys.getenv('IR_TEST_RESOLUTIONS'), append = TRUE)\n"
        )).unwrap();
        Self {
            root,
            address,
            requests,
            fault,
            server: Some(server),
        }
    }

    fn run(
        &self,
        cache: &TempPath,
        policy: Option<&str>,
        refs: &[&str],
        expression: &str,
    ) -> Output {
        self.command(cache, policy, refs)
            .args(["-e", expression])
            .output()
            .unwrap()
    }

    fn command(&self, cache: &TempPath, policy: Option<&str>, refs: &[&str]) -> Command {
        let mut command = ir();
        // pkgcache also requests optional supplemental metadata. Route any
        // external request to the fixture's rejecting proxy, so these tests
        // work offline and can never consult a live CRAN/PPM release.
        for name in [
            "http_proxy",
            "https_proxy",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
        ] {
            command.env(name, format!("http://{}", self.address));
        }
        command
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost");
        for (variable, subdir) in [
            ("IR_CACHE_DIR", "ir"),
            ("R_USER_CACHE_DIR", "r"),
            ("XDG_CACHE_HOME", "xdg"),
            ("RENV_PATHS_ROOT", "renv"),
            ("RENV_PATHS_CACHE", "renv/cache"),
            ("RENV_PATHS_SOURCE", "renv/source"),
            ("RENV_PATHS_BINARY", "renv/binary"),
            ("RENV_PATHS_CELLAR", "renv/cellar"),
            ("PKG_CACHE_DIR", "pak/downloads"),
            ("PKG_PACKAGE_CACHE_DIR", "pak/packages"),
            ("PKG_METADATA_CACHE_DIR", "pak/metadata"),
        ] {
            command.env(variable, cache.join(subdir));
        }
        let tmp = cache.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        command
            .env("TMPDIR", &tmp)
            .env("TMP", &tmp)
            .env("TEMP", &tmp)
            .env("R_PROFILE_USER", self.root.join("profile.R"))
            .env("R_ENVIRON_USER", self.root.join("absent"))
            .env("R_PROFILE", self.root.join("absent"))
            .env("R_ENVIRON", self.root.join("absent"))
            .env("R_LIBS_USER", self.root.join("tooling"))
            .env("R_LIBS_SITE", "NULL")
            .env_remove("R_LIBS")
            .env("PKG_USE_BIOCONDUCTOR", "false")
            .env("PKG_HTTP_RETRY", "false")
            .env("PKG_EXTRA_ARCH_REPOS", "true")
            .env("IR_TEST_RESOLUTIONS", cache.join("resolver-starts"))
            .env_remove("IR_PREFER_BINARIES")
            .env_remove("PKG_PLATFORMS")
            .args(["run", "--isolated", "--vanilla"]);
        if let Some(policy) = policy {
            command.env("IR_PREFER_BINARIES", policy);
        }
        for package in refs {
            command.args(["--with", package]);
        }
        command
    }

    fn binary_dir(&self) -> PathBuf {
        self.root.join(
            fs::read_to_string(self.root.join("binary-path"))
                .unwrap()
                .trim(),
        )
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let mut stream = TcpStream::connect(self.address).unwrap();
        stream.write_all(b"GET /stop HTTP/1.1\r\n\r\n").unwrap();
        self.server.take().unwrap().join().unwrap();
    }
}

#[test]
fn default_prefers_older_binary_with_source_only_dependency() {
    let repo = Repository::new();
    let cache = temp_dir("ir-binary-default-cache");
    let out = repo.run(&cache, None, &["irlag"],
        "stopifnot(packageVersion('irlag') == '1.0.0', irlag::artifact() == 'binary', irsourceonly::artifact() == 'source'); cat('binary-with-source-dependency\\n')");
    assert_success(&out);
    assert_stdout_contains(&out, "binary-with-source-dependency");
    let requests = repo.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|p| p.contains("/bin/") && p.contains("irlag_1.0.0")),
        "{requests:?}"
    );
    assert!(
        !requests.iter().any(|p| p.contains("src/contrib/irlag_")),
        "{requests:?}"
    );
}

#[test]
fn binary_policy_modes_constraints_and_transitive_dependencies() {
    let repo = Repository::new();
    for (policy, refs, expression) in [
        (Some("1"), vec!["irlag"], "stopifnot(irlag::artifact() == 'binary', packageVersion('irlag') == '1.0.0')"),
        (Some("0"), vec!["irlag"], "stopifnot(irlag::artifact() == 'source', packageVersion('irlag') == '2.0.0')"),
        (Some("0"), vec!["irsame"], "stopifnot(irsame::artifact() == 'binary')"),
        (None, vec!["irparent"], "stopifnot(irlag::artifact() == 'binary')"),
        (None, vec!["irminimum"], "stopifnot(irlag::artifact() == 'source', packageVersion('irlag') == '2.0.0')"),
        (None, vec!["irlag@2.0.0"], "stopifnot(irlag::artifact() == 'source')"),
        (None, vec!["irlag@>=2.0.0"], "stopifnot(irlag::artifact() == 'source')"),
        (None, vec!["irlag>=1.0.0", "irlag>=2.0.0"], "stopifnot(irlag::artifact() == 'source', packageVersion('irlag') == '2.0.0')"),
        (None, vec!["irconflict", "irsourceonly@1.0.0"], "stopifnot(irconflict::artifact() == 'source', packageVersion('irconflict') == '2.0.0')"),
        (None, vec!["irchoice", "irsourceonly@1.0.0"], "stopifnot(irchoice::artifact() == 'source', packageVersion('irchoice') == '2.0.0')"),
        (None, vec!["irlag?source"], "stopifnot(irlag::artifact() == 'source', packageVersion('irlag') == '2.0.0')"),
    ] {
        let cache = temp_dir("ir-binary-policy-case");
        assert_success(&repo.run(&cache, policy, &refs, expression));
    }
}

#[test]
fn unavailable_binary_resolves_again_with_source_fallback() {
    let repo = Repository::new();
    for entry in fs::read_dir(repo.binary_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("irlag_")
        {
            fs::remove_file(path).unwrap();
        }
    }
    let cache = temp_dir("ir-binary-unavailable");
    let out = repo.run(&cache, None, &["irlag"],
        "stopifnot(packageVersion('irlag') == '2.0.0', irlag::artifact() == 'source', irsourceonly::artifact() == 'source', irnewonly::artifact() == 'source')");
    assert_success(&out);
}

#[test]
fn unavailable_binary_falls_back_to_source_without_searching_older_binaries() {
    let repo = Repository::new();
    *repo.fault.lock().unwrap() = Some(("irlag_1.0.0".to_string(), 410));
    let cache = temp_dir("ir-binary-alternative");
    assert_success(&repo.run(
        &cache,
        None,
        &["irlag"],
        "stopifnot(packageVersion('irlag') == '2.0.0', irlag::artifact() == 'source', irnewonly::artifact() == 'source')",
    ));
    let binary_path = fs::read_to_string(repo.root.join("binary-path")).unwrap();
    *repo.fault.lock().unwrap() = Some((format!("/{}/iridentity_", binary_path.trim()), 410));
    let cache = temp_dir("ir-binary-pinned-fallback");
    assert_success(&repo.run(
        &cache,
        None,
        &["iridentity@1.0.0"],
        "stopifnot(packageVersion('iridentity') == '1.0.0', iridentity::artifact() == 'source')",
    ));
    *repo.fault.lock().unwrap() = Some((format!("/{}/irlag_", binary_path.trim()), 410));
    let cache = temp_dir("ir-binary-older-pin-fallback");
    repo.requests.lock().unwrap().clear();
    let out = repo.run(&cache, None, &["irlag@1.0.0"], "stop('must not run')");
    assert!(!out.status.success());
    // pak cannot resolve this older source pin from the fixture. Keep its
    // resolution error rather than silently upgrading the pinned package.
    assert!(
        output_text(&out).contains("Could not solve package dependencies"),
        "{}",
        output_text(&out)
    );
    assert!(!cache.join("ir/resolutions").exists());
    assert!(!repo
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.contains("src/contrib/irlag_2.0.0")));
}

#[test]
fn binary_authentication_and_installation_errors_are_not_source_fallbacks() {
    let repo = Repository::new();
    *repo.fault.lock().unwrap() = Some(("irlag_1.0.0".to_string(), 401));
    let cache = temp_dir("ir-binary-authentication");
    let out = repo.run(&cache, None, &["irlag"], "stop('must not run')");
    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(output_text(&out).contains("401"), "{}", output_text(&out));
    assert!(!cache.join("ir/resolutions").exists());
    assert!(!repo
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.contains("src/contrib/irlag_")));
    *repo.fault.lock().unwrap() = None;
    let out = repo.run(&cache, None, &["irsame?source"], "stop('must not run')");
    assert!(!out.status.success(), "{}", output_text(&out));
    assert!(
        output_text(&out).contains("IR_UNEXPECTED_SOURCE_ARTIFACT"),
        "{}",
        output_text(&out)
    );
    assert!(!cache.join("ir/resolutions").exists());
    assert_success(&repo.run(
        &cache,
        None,
        &["irsame"],
        "stopifnot(irsame::artifact() == 'binary')",
    ));
}

#[test]
fn explicit_artifact_and_local_sources_and_unsatisfiable_constraints() {
    let repo = Repository::new();
    let cache = temp_dir("ir-explicit-artifacts");
    let url = format!(
        "url::http://{}/repo/src/contrib/irlag_2.0.0.tar.gz",
        repo.address
    );
    let local = format!(
        "local::{}",
        renviron_path(&repo.root.join("packages/irlag-2.0.0-source/irlag"))
    );
    for package in [&url, &local] {
        assert_success(&repo.run(
            &cache,
            None,
            &[package],
            "stopifnot(packageVersion('irlag') == '2.0.0', irlag::artifact() == 'source')",
        ));
    }
    let cache = temp_dir("ir-unsatisfiable-binary-policy");
    for package in ["irlag>=9.0.0", "irconflict@1.0.0"] {
        let out = repo.run(
            &cache,
            None,
            &[package, "irsourceonly@1.0.0"],
            "stop('must not run')",
        );
        assert!(!out.status.success(), "{}", output_text(&out));
        assert!(
            output_text(&out).contains("Could not solve package dependencies"),
            "{}",
            output_text(&out)
        );
        assert!(!cache.join("ir/resolutions").exists());
    }
}

#[test]
fn binary_policy_cache_identity_and_reuse() {
    let repo = Repository::new();
    let cache = temp_dir("ir-binary-policy-cache");
    // The old Rust key format is frozen here to exercise the cache transition
    // through the CLI, rather than testing the new key helper against itself.
    let executable = fs::canonicalize(PathBuf::from(rscript())).unwrap();
    let meta = fs::metadata(&executable).unwrap();
    let mut identity = format!(
        "{};len={};mtime={}",
        executable.to_string_lossy(),
        meta.len(),
        meta.modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    for name in ["R_ARCH", "R_HOME"] {
        if let Some(value) = std::env::var_os(name) {
            identity.push_str(&format!(";{name}={}", value.to_string_lossy()));
        }
    }
    let fields = [
        "irlag".to_string(),
        "latest".to_string(),
        format!("rscript: {identity}"),
    ];
    let encoded = fields
        .iter()
        .map(|f| format!("{}:{f}\n", f.len()))
        .collect::<String>();
    let old_key = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let old_library = cache.join("legacy-library");
    fs::create_dir_all(&old_library).unwrap();
    fs::create_dir_all(cache.join("ir/resolutions")).unwrap();
    let legacy = format!(
        "latest: {}\n{}\n",
        current_utc_seconds(),
        renviron_path(&old_library)
    );
    fs::write(cache.join("ir/resolutions").join(old_key), legacy).unwrap();
    let mut binary_library = PathBuf::new();
    for (policy, starts) in [
        (None, 1),
        (Some("1"), 1),
        (Some("0"), 2),
        (Some("0"), 2),
        (None, 2),
    ] {
        let binary = policy != Some("0");
        let expression = format!(
            "stopifnot(irlag::artifact() == '{}'); cat('IR_LIBRARY=', normalizePath(.libPaths()[1], winslash = '/'), '\\n', sep = '')",
            if binary { "binary" } else { "source" }
        );
        let out = repo.run(&cache, policy, &["irlag"], &expression);
        assert_success(&out);
        if binary {
            binary_library = PathBuf::from(
                stdout(&out)
                    .lines()
                    .find_map(|line| line.strip_prefix("IR_LIBRARY="))
                    .unwrap(),
            );
        }
        assert_eq!(
            fs::read_to_string(cache.join("resolver-starts"))
                .unwrap()
                .lines()
                .count(),
            starts
        );
    }
    assert_eq!(
        fs::read_dir(cache.join("ir/resolutions")).unwrap().count(),
        3
    );
    let out = repo
        .command(&cache, None, &["irlag"])
        .env("PKG_PLATFORMS", "source")
        .args([
            "-e",
            "stopifnot(irlag::artifact() == 'source', packageVersion('irlag') == '2.0.0')",
        ])
        .output()
        .unwrap();
    assert_success(&out);
    assert_eq!(
        fs::read_to_string(cache.join("resolver-starts"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    fs::write(binary_library.join(".ir-incomplete"), "interrupted").unwrap();
    assert_success(&repo.run(
        &cache,
        None,
        &["irlag"],
        "stopifnot(irlag::artifact() == 'binary')",
    ));
    assert!(!binary_library.join(".ir-incomplete").exists());
    assert_eq!(
        fs::read_to_string(cache.join("resolver-starts"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    let out = repo.run(&cache, Some("invalid"), &["irlag"], "stop('must not run')");
    assert!(!out.status.success());
    assert!(output_text(&out).contains("IR_PREFER_BINARIES must be 0 or 1"));
    assert_eq!(
        fs::read_to_string(cache.join("resolver-starts"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    for (package, artifact) in [("iridentity?source", "source"), ("iridentity", "binary")] {
        assert_success(&repo.run(
            &cache,
            None,
            &[package],
            &format!("stopifnot(packageVersion('iridentity') == '1.0.0', iridentity::artifact() == '{artifact}')"),
        ));
    }
}
