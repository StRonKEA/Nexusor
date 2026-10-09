let permissionRequested = false;

export function ensureNotificationPermission() {
  if (typeof window !== "undefined" && "Notification" in window && !permissionRequested) {
    permissionRequested = true;
    if (Notification.permission === "default") {
      void Notification.requestPermission();
    }
  }
}

export function sendDesktopNotification(title: string, body: string) {
  if (typeof window === "undefined" || !("Notification" in window)) return;

  if (Notification.permission === "granted") {
    try {
      new Notification(title, {
        body,
        icon: "/app-icon.png",
      });
    } catch {
      // Ignored
    }
  } else if (Notification.permission === "default") {
    void Notification.requestPermission().then((perm) => {
      if (perm === "granted") {
        try {
          new Notification(title, {
            body,
            icon: "/app-icon.png",
          });
        } catch {
          // Ignored
        }
      }
    });
  }
}
