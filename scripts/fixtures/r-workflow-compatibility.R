# Owned synthetic fixture for R workflow UI verification.
library(ggplot2)
library(pheatmap)

workflow_data <- data.frame(
  CellLine = c("A", "B", "C", "D"),
  Description = c("first", "second", "third", "fourth"),
  setSize = c(2, 4, 6, 8),
  NES = c(-2, -1, 1, 2)
)
workflow_dot <- ggplot(workflow_data, aes(CellLine, Description, size = setSize, color = NES)) +
  geom_point() +
  scale_color_gradient2(low = "blue", mid = "white", high = "red", midpoint = 0) +
  scale_size_continuous() + theme_bw() + labs(color = "NES\nscore") +
  ggtitle("Synthetic dot plot")

workflow_matrix <- matrix(seq(-2, 2, length.out = 100), nrow = 10,
  dimnames = list(paste0("long_gene_label_", 1:10), paste0("long_sample_label_", 1:10)))
workflow_heatmap <- pheatmap(workflow_matrix, silent = TRUE,
  color = colorRampPalette(c("#0000FF", "#FFFFFF", "#FF0000"))(20))
workflow_function <- function(x) x + 1
workflow_values <- c("한글", "color", "plot")
cat("R_WORKFLOW_FIXTURE_READY\n")
