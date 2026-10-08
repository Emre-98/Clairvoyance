import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
// Inter Black: the letters of the ability bubbles (lib/bubbles.ts), bundled so it works offline.
import "@fontsource/inter/latin-900.css";

const app = mount(App, { target: document.getElementById("app")! });
export default app;
