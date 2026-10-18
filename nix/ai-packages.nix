# Keep the upstream package set and its lock; only ChatGPT needs this repair.
aiPackages:
aiPackages
// (
  if aiPackages ? chatgpt then
    {
      chatgpt = import ./chatgpt/package.nix { upstream = aiPackages.chatgpt; };
    }
  else
    { }
)
