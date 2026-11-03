import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import Toast from "./components/Toast.svelte";

const isToast = location.hash === "#toast";
if (isToast) document.body.classList.add("toast-body");

export default mount(isToast ? Toast : App, { target: document.getElementById("app")! });
