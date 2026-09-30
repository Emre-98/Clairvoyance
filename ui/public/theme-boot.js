// Runs before the first paint (a classic, blocking script in <head>), so the right theme is
// there from the very first frame: no flash when the window opens.
// Source of truth: the app injects window.__CV_THEME__ from the settings; the copy in
// localStorage covers the browser preview.
(function () {
  var t = window.__CV_THEME__;
  try {
    if (!t) t = localStorage.getItem("cv-theme");
  } catch (e) {}
  if (t !== "dark" && t !== "light") t = "system";
  document.documentElement.setAttribute("data-theme", t);
})();
