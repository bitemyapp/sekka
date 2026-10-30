// SPDX-License-Identifier: GPL-3.0-or-later
// Ananicy's worker owns this cache. child_rule names a rule for descendants,
// including unlisted executables and fork-only children. No cgroup migration.
#pragma once

#include <fstream>
#include <optional>
#include <sstream>
#include <string>
#include <unordered_map>
#include <sys/types.h>

namespace sekka {

struct ProcessIdentity {
  pid_t pid{}, parent{};
  unsigned uid{};
  unsigned long long started{};
  bool operator==(const ProcessIdentity &) const = default;
};

inline std::optional<ProcessIdentity> process_identity(pid_t pid) {
  // BPF fork events include TIDs. Resolve the thread group before caching:
  // a thread's PPid is the process's parent, not its creating thread.
  std::ifstream status("/proc/" + std::to_string(pid) + "/status");
  std::string line;
  ProcessIdentity process;
  bool have_uid = false;
  while (std::getline(status, line)) {
    std::istringstream field(line);
    std::string key;
    field >> key;
    if (key == "Tgid:") field >> process.pid;
    if (key == "Uid:") {
      unsigned effective, saved, filesystem;
      have_uid = bool(field >> process.uid >> effective >> saved >> filesystem);
      // Do not carry ancestry policy across a setuid helper's credentials.
      if (!have_uid || effective != process.uid || saved != process.uid ||
          filesystem != process.uid) return std::nullopt;
    }
  }
  if (!have_uid || process.pid <= 1) return std::nullopt;
  std::ifstream stat("/proc/" + std::to_string(process.pid) + "/stat");
  std::getline(stat, line);
  // comm can contain spaces and parentheses; field 3 starts after the last ).
  auto end = line.rfind(')');
  if (end == std::string::npos) return std::nullopt;
  std::istringstream fields(line.substr(end + 1));
  std::string ignored;
  fields >> ignored >> process.parent;
  for (int field = 5; field < 22; ++field) fields >> ignored;
  if (!(fields >> process.started)) return std::nullopt;
  return process;
}

class DescendantRules {
  struct Entry {
    ProcessIdentity process;
    std::string rule;
  };
  std::unordered_map<pid_t, Entry> entries;
  unsigned events = 0;

  static bool same_process(const ProcessIdentity &a, const ProcessIdentity &b) {
    // Reparenting is allowed; PID reuse and credentials changing are not.
    return a.pid == b.pid && a.started == b.started && a.uid == b.uid;
  }

public:
  // Injected readers also allow deterministic tests of PID reuse, reparenting,
  // credential boundaries and process trees that disappear during discovery.
  template <typename ReadProcess, typename ChildRule>
  std::string inherited(pid_t pid, ReadProcess read, ChildRule child_rule) {
    if (++events % 1024 == 0) {
      std::erase_if(entries, [&](const auto &item) {
        auto current = read(item.first);
        return !current || !same_process(*current, item.second.process);
      });
    }
    auto target = read(pid);
    if (!target) return {};
    auto current = target;
    std::string inherited_rule;
    // A bounded walk also tolerates inconsistent reads while processes exit.
    for (unsigned depth = 0; current && depth < 128; ++depth) {
      if (current->uid != target->uid) break;
      auto own = child_rule(current->pid);
      if (!own.empty()) {
        // A new root (e.g. Fossilize) overrides the outer Steam tree.
        entries[current->pid] = {*current, own};
        inherited_rule = own;
        break;
      }
      auto cached = entries.find(current->pid);
      if (cached != entries.end() &&
          same_process(*current, cached->second.process)) {
        inherited_rule = cached->second.rule;
        break;
      }
      if (current->parent <= 1 || current->parent == current->pid) break;
      current = read(current->parent);
    }
    if (!inherited_rule.empty()) {
      // Cache only the target. Caching ancestors from a descendant would
      // incorrectly spread its remembered policy to unrelated siblings.
      entries[target->pid] = {*target, inherited_rule};
    }
    return inherited_rule;
  }
};

} // namespace sekka
