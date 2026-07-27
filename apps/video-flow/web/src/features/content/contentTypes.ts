/** Prop dùng chung cho các trang quản lý nội dung project. */
export type OpenPipelineProp = {
  onOpenPipeline?: (
    projectId: string,
    opts?: { videoId?: string }
  ) => void;
};
