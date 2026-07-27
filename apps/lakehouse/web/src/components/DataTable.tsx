import { Table } from 'antd'
import type { TableProps } from 'antd'

// Bảng dày, phân trang 20, footer "1-N của N" — dùng chung mọi trang list.
export function DataTable<R extends object>(props: TableProps<R>) {
  return (
    <Table<R>
      size="small"
      pagination={{
        defaultPageSize: 20,
        pageSizeOptions: [10, 20, 50, 100],
        showSizeChanger: true,
        size: 'small',
        showTotal: (total, range) => `${range[0]}-${range[1]} của ${total}`,
      }}
      scroll={{ x: 'max-content' }}
      {...props}
    />
  )
}
