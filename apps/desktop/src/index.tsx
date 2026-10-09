import ReactDOM from "react-dom/client";
import "../node_modules/monaco-editor/min/vs/editor/editor.main.css";
import { I18nRoot } from "./i18n/I18nRoot";
import { initializeI18n } from "./i18n/store";
import { applyTheme } from "./shared/theme/theme";
import "./styles/globals.scss";

initializeI18n();
applyTheme();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <I18nRoot />,
);
