{ pkgs, chatgpt }:
pkgs.runCommand "chatgpt-executor-plugins" { nativeBuildInputs = [ pkgs.nodejs ]; } ''
  node ${../../tests/test_chatgpt_plugins.cjs} ${chatgpt.unwrapped}/lib/chatgpt/resources
  touch "$out"
''
