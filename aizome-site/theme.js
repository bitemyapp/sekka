// The menu bar's Night/Day switch, remembered. The choice is kept in this
// browser's localStorage under "aizome-theme" and is never sent anywhere.
// The page loads this in its head, so a remembered Day is applied before
// anything is drawn. Without it the switch still works, on its page only.
(function () {
  "use strict";
  var key = "aizome-theme";
  var root = document.documentElement;

  function remembered() {
    try {
      return localStorage.getItem(key) === "day" ? "day" : "night";
    } catch (e) {
      return "night";
    }
  }

  function show(theme) {
    if (theme === "day") {
      root.setAttribute("data-theme", "day");
    } else {
      root.removeAttribute("data-theme");
    }
    var input = document.getElementById("site-theme");
    if (input) {
      input.checked = theme === "day";
    }
  }

  show(remembered());

  document.addEventListener("DOMContentLoaded", function () {
    var input = document.getElementById("site-theme");
    if (!input) {
      return;
    }
    show(remembered());
    input.addEventListener("change", function () {
      var theme = input.checked ? "day" : "night";
      try {
        localStorage.setItem(key, theme);
      } catch (e) {
        // Storage refused (a private window, say): this page still changes.
      }
      show(theme);
    });
  });

  // Another tab changed it, or the page came back from the back/forward
  // cache with the switch as it was.
  window.addEventListener("storage", function (event) {
    if (event.key === key) {
      show(remembered());
    }
  });
  window.addEventListener("pageshow", function () {
    show(remembered());
  });
})();
