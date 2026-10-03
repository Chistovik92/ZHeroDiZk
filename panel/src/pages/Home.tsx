import { api } from "../api";
import { useApp, useLoad } from "../app-context";
import { Async, Card, Page } from "../ui";

export function Home() {
  const { t, org } = useApp();
  const state = useLoad(async () => {
    const [devices, groups, grants] = await Promise.all([api.devices(org.id), api.groups(org.id), api.grants(org.id)]);
    return {
      devices: devices.filter((d) => d.status === "active").length,
      groups: groups.length,
      grants: grants.filter((g) => g.status === "active").length,
    };
  }, [org.id]);
  return (
    <Page title={t("homeTitle")}>
      <Async state={state}>
        {(s) => (
          <div className="stats">
            <div className="stat"><b>{s.devices}</b><span>{t("homeDevices")}</span></div>
            <div className="stat"><b>{s.groups}</b><span>{t("homeGroups")}</span></div>
            <div className="stat"><b>{s.grants}</b><span>{t("homeActive")}</span></div>
          </div>
        )}
      </Async>
      <Card>
        <p className="muted">{t("homeNote")}</p>
      </Card>
    </Page>
  );
}
