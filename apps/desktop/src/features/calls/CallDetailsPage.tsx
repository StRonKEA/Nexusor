import { useEffect, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api, type CallDetail } from "../../shared/api";
import { CallDetails } from "./CallDetails";
import { PageContent } from "../../shell/layout/PageContent";
import { PageActions } from "../../shell/PageActions";
import { TitledCard } from "../../shared/ui/TitledCard";
import styles from "./CallDetailsPage.module.scss";

export function CallDetailsPage() {
  const { callId = "" } = useParams();
  const [detail, setDetail] = useState<CallDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setError(null);
    void api.call(callId).then((value) => {
      if (!cancelled) setDetail(value);
    }).catch((cause) => {
      if (!cancelled) setError(cause instanceof Error ? cause.message : String(cause));
    });
    return () => {
      cancelled = true;
    };
  }, [callId]);

  const content = error
    ? <TitledCard title={t("calls.unable_to_load_call_details")}><div className={styles.panel}>{error}</div></TitledCard>
    : detail
      ? <CallDetails detail={detail} />
      : <TitledCard title={t("calls.call_details")}><div className={styles.panel}>{t("calls.loading_call_details")}</div></TitledCard>;

  return <>
    <PageActions position="left">
      <Link className={styles.backLink} to="/calls">{t("calls.back_to_calls")}</Link>
    </PageActions>
    <PageContent
      title={detail?.call.display_name ?? t("calls.call_details")}
      sections={[{ key: "call-detail", estimatedHeight: 900, content }]}
    />
  </>;
}
