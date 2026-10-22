import { mount } from "svelte";
import App from "./App.svelte";
import { store } from "./store.svelte";
import "./style.css";

document.documentElement.lang = store.lang;
mount(App, { target: document.getElementById("app")! });
