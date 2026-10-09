import { HashRouter, Navigate, Route, Routes, useParams } from "react-router-dom";
import { useEffect, useRef } from "react";
import { TooltipProvider } from "./shared/ui/Tooltip";
import { MessageProvider } from "./shared/ui/MessageProvider";
import { useMessage } from "./shared/ui/message";
import { AppFrame } from "./shell/AppFrame";
import { AppLayout } from "./shell/AppLayout";
import { CallsPage } from "./features/calls/CallsPage";
import { CallDetailsPage } from "./features/calls/CallDetailsPage";
import { CombosPage } from "./features/combos/CombosPage";
import { CursorSettingsPage } from "./features/models/CursorSettingsPage";
import { CursorIntegrationPage } from "./features/cursor/CursorIntegrationPage";
import { HomePage } from "./features/home/HomePage";
import { ProvidersPage } from "./features/providers/ProvidersPage";
import { ProviderDetailPage } from "./features/providers/ProviderDetailPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { useAppStore } from "./shared/store/appStore";

export function App() {
  return (
    <TooltipProvider>
      <HashRouter>
        <Routes>
          <Route element={<AppFrame />}>
            <Route element={<AppLayout />}>
              <Route index element={<HomePage />} />
              <Route path="cursor" element={<CursorIntegrationPage />} />
              <Route path="providers" element={<ProvidersPage />} />
              <Route path="providers/:pluginId" element={<ProviderDetailPage />} />
              <Route path="models" element={<CursorSettingsPage />} />
              <Route path="combos" element={<CombosPage />} />
              <Route path="calls" element={<CallsPage />} />
              <Route path="calls/:callId" element={<CallDetailsPage />} />
              <Route path="settings" element={<SettingsPage />} />
              <Route path="harness/cursor" element={<Navigate to="/cursor" replace />} />
              <Route path="plugins" element={<Navigate to="/providers" replace />} />
              <Route path="plugins/:pluginId" element={<PluginRedirect />} />
            </Route>
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
      </HashRouter>
      <AppMessages />
    </TooltipProvider>
  );
}

function PluginRedirect() {
  const { pluginId } = useParams();
  return <Navigate to={pluginId ? `/providers/${pluginId}` : "/providers"} replace />;
}

function AppMessages() {
  const { error } = useAppStore();
  const previousError = useRef<string | null>(null);
  const showMessage = useMessage();

  useEffect(() => {
    if (error && error !== previousError.current) showMessage(error);
    previousError.current = error;
  }, [error, showMessage]);

  return <MessageProvider />;
}
