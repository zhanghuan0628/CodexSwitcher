type MetricTileTone = "default" | "success" | "warning" | "danger";

type MetricTileProps = {
  label: string;
  value: string;
  tone?: MetricTileTone;
  detail?: string;
};

export function MetricTile({ label, value, tone = "default", detail }: MetricTileProps) {
  const compactClass = value.length > 22 ? "metric-tile__value metric-tile__value--dense" : "metric-tile__value";

  return (
    <div className={`metric-tile metric-tile--${tone}`}>
      <span className="metric-tile__label">{label}</span>
      <strong className={compactClass}>{value}</strong>
      {detail ? <p className="metric-tile__detail">{detail}</p> : null}
    </div>
  );
}

export type { MetricTileProps, MetricTileTone };
