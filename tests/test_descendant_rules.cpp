// SPDX-License-Identifier: GPL-3.0-or-later
#include "descendant-rules.hpp"
#include <cassert>
#include <iostream>
#include <map>
#include <thread>
#include <unistd.h>

int main() {
  using sekka::ProcessIdentity;
  std::map<pid_t, ProcessIdentity> processes;
  std::map<pid_t, std::string> roots;
  auto add = [&](pid_t pid, pid_t parent, unsigned uid = 1000,
                 unsigned long long started = 100) {
    processes[pid] = {pid, parent, uid, started};
  };
  auto read = [&](pid_t pid) -> std::optional<ProcessIdentity> {
    auto it = processes.find(pid);
    if (it == processes.end()) return std::nullopt;
    return it->second;
  };
  auto child_rule = [&](pid_t pid) { return roots[pid]; };
  sekka::DescendantRules policy;
  auto classify = [&](pid_t pid) { return policy.inherited(pid, read, child_rule); };

  add(10, 1);
  roots[10] = "game";
  add(11, 10); // An unlisted wrapper, before any event for Steam was handled.
  add(12, 11);
  assert(classify(12) == "game");
  assert(classify(11) == "game");

  add(20, 10);
  roots[20] = "shader";
  add(21, 20);
  assert(classify(21) == "shader");
  processes[21].parent = 10; // A subreaper adopts the shader helper.
  assert(classify(21) == "shader");
  add(22, 21);
  assert(classify(22) == "shader");
  processes[12].parent = 1;
  assert(classify(12) == "game");
  add(13, 12);
  assert(classify(13) == "game");

  add(30, 10, 0); // Privileged helpers and other users are boundaries.
  add(31, 10, 1001);
  assert(classify(30).empty());
  assert(classify(31).empty());
  add(12, 1, 1000, 101); // Reuse of an observed game PID.
  assert(classify(12).empty());
  add(21, 1, 1001); // Same PID/start time, changed credentials.
  assert(classify(21).empty());
  assert(classify(999).empty());

  add(40, 1);
  assert(classify(40).empty());
  roots[40] = "game"; // Exec into Steam after a negative lookup.
  add(41, 40);
  assert(classify(41) == "game");
  roots[41] = "shader"; // A game descendant execs the background compiler.
  assert(classify(41) == "shader");
  roots.erase(41); // Its next exec retains its background lineage.
  assert(classify(41) == "shader");

  // Do not propagate a remembered orphan's policy upward to its adopter.
  add(50, 1);
  processes[41].parent = 50;
  assert(classify(41) == "shader");
  add(51, 50);
  assert(classify(51).empty());
  add(60, 61); // Racy or malformed cyclic ancestry is bounded.
  add(61, 60);
  assert(classify(60).empty());

  // Exercise cache pruning after process exit and PID reuse.
  processes.erase(10);
  roots.erase(10);
  for (int i = 0; i < 2048; ++i) assert(classify(999).empty());
  add(10, 1, 1000, 102);
  assert(classify(10).empty());

  // Real /proc parsing resolves non-leader threads to their process identity.
  auto identity = sekka::process_identity(getpid());
  assert(identity && identity->pid == getpid() && identity->parent == getppid());
  std::thread thread([&] {
    auto task = sekka::process_identity(gettid());
    assert(task && *task == *identity);
  });
  thread.join();
  std::cout << "Descendant policy: ancestry, exec, reparenting, credentials, PID reuse, threads passed\n";
}
