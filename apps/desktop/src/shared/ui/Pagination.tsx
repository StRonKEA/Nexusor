import controls from "./Controls.module.scss";
import { Icon } from "./Icon";
import { chevronDoubleLeftIcon, chevronDoubleRightIcon, chevronLeftIcon, chevronRightIcon } from "./icons";
import { Select } from "./Select";
import { TooltipTrigger } from "./TooltipTrigger";
import styles from "./Pagination.module.scss";

export function Pagination({ page, pageCount, pageSize, total, pageSizes = [20, 50, 100], onPageChange, onPageSizeChange }: {
  page: number;
  pageCount: number;
  pageSize: number;
  total: number;
  pageSizes?: number[];
  onPageChange: (page: number) => void;
  onPageSizeChange: (pageSize: number) => void;
}) {
  const disabledPrevious = page <= 1;
  const disabledNext = page >= pageCount;
  return <div className={styles.root}>
    <span>{t("common.total", { count: total })}</span>
    <div className={styles.controls}>
      <div className={styles.pageSize}><Select ariaLabel={t("common.items_per_page")} value={String(pageSize)} options={pageSizes.map((size) => ({ value: String(size), label: t("common.per_page", { count: size }) }))} onChange={(value) => onPageSizeChange(Number(value))} /></div>
      <span>{t("common.page_of", { page, count: pageCount })}</span>
      <TooltipTrigger label={t("common.first_page")}><button className={controls.iconButton} aria-label={t("common.first_page")} disabled={disabledPrevious} onClick={() => onPageChange(1)}><Icon icon={chevronDoubleLeftIcon} size="1.1em" /></button></TooltipTrigger>
      <TooltipTrigger label={t("common.previous_page")}><button className={controls.iconButton} aria-label={t("common.previous_page")} disabled={disabledPrevious} onClick={() => onPageChange(page - 1)}><Icon icon={chevronLeftIcon} size="1.1em" /></button></TooltipTrigger>
      <TooltipTrigger label={t("common.next_page")}><button className={controls.iconButton} aria-label={t("common.next_page")} disabled={disabledNext} onClick={() => onPageChange(page + 1)}><Icon icon={chevronRightIcon} size="1.1em" /></button></TooltipTrigger>
      <TooltipTrigger label={t("common.last_page")}><button className={controls.iconButton} aria-label={t("common.last_page")} disabled={disabledNext} onClick={() => onPageChange(pageCount)}><Icon icon={chevronDoubleRightIcon} size="1.1em" /></button></TooltipTrigger>
    </div>
  </div>;
}
