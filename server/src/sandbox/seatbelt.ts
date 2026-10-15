import {
    aboveLocked,
    canon,
    hostEnv,
    type Policy,
    tempRoot,
} from "./policy.ts";
import type { Confined, Sandbox } from "./sandbox.ts";

const SANDBOX_EXEC = "/usr/bin/sandbox-exec";

// A deny-default profile after the base policy of OpenAI Codex.
const BASE = `(version 1)
(deny default)

; children inherit the policy
(allow process-exec)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))

; stat works everywhere so path traversal and getcwd succeed
(allow file-read-metadata)

(allow sysctl-read
  (sysctl-name-prefix "hw.")
  (sysctl-name-prefix "kern.")
  (sysctl-name-prefix "vm.")
  (sysctl-name-prefix "machdep.cpu.")
  (sysctl-name-prefix "net.routetable.")
  (sysctl-name "sysctl.proc_cputype"))
(allow sysctl-write (sysctl-name "kern.grade_cputype"))

(allow iokit-open (iokit-registry-entry-class "RootDomainUserClient"))
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.PowerManagement.control")
  (global-name "com.apple.cfprefsd.daemon")
  (global-name "com.apple.cfprefsd.agent"))
(allow ipc-posix-sem)

; devices
(allow file-read* file-write* file-ioctl
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/dtracehelper")
  (literal "/dev/tty"))
(allow file-read* (literal "/dev/random") (literal "/dev/urandom"))
(allow pseudo-tty)
(allow file-read* file-write* file-ioctl (literal "/dev/ptmx"))
(allow file-read* file-write* file-ioctl (regex #"^/dev/ttys[0-9]+"))

; system locations needed to run ordinary binaries
(allow file-read*
  (subpath "/usr")
  (subpath "/bin")
  (subpath "/sbin")
  (subpath "/System")
  (subpath "/Library/Apple")
  (subpath "/Library/Developer")
  (subpath "/Library/Preferences/Logging")
  (literal "/Library/Preferences/com.apple.dt.Xcode.plist")
  (subpath "/opt/homebrew")
  (subpath "/private/etc")
  (subpath "/private/var/db/timezone")
  (subpath "/private/var/db/dyld")
  (subpath "/private/var/db/xcode_select_link")
  (subpath "/Applications/Xcode.app/Contents")
  (literal "/")
  (literal "/dev"))
`;

const NET = `
(allow network*)
(allow system-socket)
(allow mach-lookup
  (global-name "com.apple.dnssd.service")
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration")
  (global-name "com.apple.SystemConfiguration.helper")
  (global-name "com.apple.networkd")
  (global-name "com.apple.nsurlsessiond")
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.securityd")
  (global-name "com.apple.SecurityServer")
  (global-name "com.apple.ocspd"))
(allow file-read*
  (subpath "/private/var/db/mds")
  (subpath "/private/var/db/DetachedSignatures")
  (subpath "/Library/Keychains")
  (subpath "/private/var/protected/trustd"))
`;

type Profile = { sbpl: string; params: [string, string][] };

/** Adds `rule` once for each of `paths` as written and once as it really is, with `{}` standing for the parameter's name. */
function rules(
    profile: Profile,
    kind: string,
    rule: string,
    paths: readonly string[],
): void {
    paths.forEach((path, i) => {
        for (const [suffix, value] of [
            ["", path],
            ["_REAL", canon(path)],
        ] as const) {
            const name = `${kind}${i}${suffix}`;
            profile.sbpl += `${rule.replace("{}", name)}\n`;
            profile.params.push([name, value]);
        }
    });
}

/** The profile text and the `-D` parameters it reads, which keep paths from needing escapes. Of two rules that match, the later one decides. */
function profile(policy: Policy, tmp: string): Profile {
    const out: Profile = { sbpl: BASE, params: [] };
    if (policy.open) {
        out.sbpl += "(allow file-read*)\n";
        // Another house's temp dir is as private as the house. The base
        // profile allows metadata by its own name, which this does not
        // undo: a path through a hidden folder to a granted one can still
        // be walked.
        rules(
            out,
            "HIDDEN",
            `(deny file-read* file-write* (subpath (param "{}")))`,
            [...policy.hidden, tempRoot()],
        );
    }
    rules(out, "TMP", `(allow file-read* file-write* (subpath (param "{}")))`, [
        tmp,
    ]);
    rules(out, "RO", `(allow file-read* (subpath (param "{}")))`, policy.ro);
    rules(
        out,
        "RW",
        `(allow file-read* file-write* (subpath (param "{}")))`,
        policy.rw,
    );
    rules(
        out,
        "LOCKED",
        `(deny file-write* (subpath (param "{}")))`,
        policy.locked,
    );
    rules(
        out,
        "ABOVE",
        `(deny file-write-unlink (literal (param "{}")))`,
        aboveLocked(policy),
    );
    if (policy.net) {
        out.sbpl += NET;
    } else {
        for (const { port } of policy.services) {
            out.sbpl += `(allow network-outbound (remote ip "localhost:${port}"))\n`;
        }
    }
    return out;
}

/** macOS Seatbelt, through `sandbox-exec`. */
export const Seatbelt: Sandbox = {
    name: "seatbelt",
    probe: {
        argv: [
            SANDBOX_EXEC,
            "-p",
            "(version 1)(allow default)",
            "--",
            "/usr/bin/true",
        ],
        hint: `macOS sandboxing needs ${SANDBOX_EXEC}`,
    },
    baseEnv: hostEnv,
    command(policy, argv, _cwd, tmp): Confined {
        const { sbpl, params } = profile(policy, tmp);
        return {
            argv: [
                SANDBOX_EXEC,
                ...params.flatMap(([name, value]) => [
                    "-D",
                    `${name}=${value}`,
                ]),
                "-p",
                sbpl,
                "--",
                ...argv,
            ],
            env: { ...policy.env, TMPDIR: tmp },
        };
    },
};
