import { Table } from 'antd'
import type { TableProps } from 'antd'
import type { T } from '../i18n'

/// Dense sortable table with the reference CRM's `1-N of N` + page-size footer.
/// A thin wrapper over AntD's Table so every list page gets identical geometry
/// and pagination copy without repeating the config.
export function DataTable<R extends object>({
  t,
  ...props
}: TableProps<R> & { t: T }) {
  return (
    <Table<R>
      size="small"
      className="data-table"
      rowKey={(r: any) => r.id ?? r.key}
      pagination={{
        defaultPageSize: 20,
        pageSizeOptions: [10, 20, 50, 100],
        showSizeChanger: true,
        size: 'small',
        showTotal: (total, range) => `${range[0]}-${range[1]} ${t('of')} ${total}`,
      }}
      {...props}
    />
  )
}
