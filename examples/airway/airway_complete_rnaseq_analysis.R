# R Vector Studio - Complete airway RNA-seq analysis
# Real public data: Bioconductor airway (Himes et al., 2014)
# Comparison: dexamethasone treated (trt) vs untreated (untrt), controlling for cell line
# Outputs: DEG table, PCA, MA, volcano, heatmap, Hallmark GSEA dot plot, SVG/PNG figures
# This script is for reproducible learning and app testing, not clinical interpretation.

# ------------------------ 0. Configuration ------------------------
INSTALL_MISSING <- FALSE          # Set FALSE when packages are preinstalled
RUN_GSEA <- TRUE                 # Requires fgsea and msigdbr
OUTPUT_DIR <- file.path(getwd(), "airway_analysis_results")
dir.create(OUTPUT_DIR, showWarnings = FALSE, recursive = TRUE)
set.seed(2026)

cran_packages <- c("ggplot2", "pheatmap", "svglite", "msigdbr")
bioc_packages <- c("airway", "DESeq2", "SummarizedExperiment", "fgsea", "BiocParallel")

if (INSTALL_MISSING) {
  missing_cran <- cran_packages[!vapply(cran_packages, requireNamespace, logical(1), quietly = TRUE)]
  if (length(missing_cran)) install.packages(missing_cran)
  missing_bioc <- bioc_packages[!vapply(bioc_packages, requireNamespace, logical(1), quietly = TRUE)]
  if (length(missing_bioc)) {
    if (!requireNamespace("BiocManager", quietly = TRUE)) install.packages("BiocManager")
    BiocManager::install(missing_bioc, ask = FALSE, update = FALSE)
  }
}
required <- c("airway", "DESeq2", "SummarizedExperiment", "ggplot2", "pheatmap")
missing <- required[!vapply(required, requireNamespace, logical(1), quietly = TRUE)]
if (length(missing)) stop("Missing required packages: ", paste(missing, collapse = ", "))

library(airway)
library(DESeq2)
library(ggplot2)
library(pheatmap)

# ------------------------ 1. Load counts and metadata ------------------------
data("airway", package = "airway")
count_matrix <- SummarizedExperiment::assay(airway)
sample_info <- as.data.frame(SummarizedExperiment::colData(airway))
sample_info$dex <- relevel(factor(sample_info$dex), ref = "untrt")
sample_info$cell <- factor(sample_info$cell)
stopifnot(identical(colnames(count_matrix), rownames(sample_info)))
cat("RNA-seq dataset:", nrow(count_matrix), "genes x", ncol(count_matrix), "samples\n")
print(table(sample_info$dex))
write.csv(sample_info, file.path(OUTPUT_DIR, "sample_metadata.csv"))

# ------------------------ 2. DESeq2: paired/blocked design ------------------------
# Control for the airway donor/cell line, as recommended by the official workflow.
dds <- DESeq2::DESeqDataSetFromMatrix(
  countData = count_matrix,
  colData = sample_info,
  design = ~ cell + dex
)
keep <- rowSums(DESeq2::counts(dds) >= 10) >= 2
dds <- dds[keep, ]
dds <- DESeq2::DESeq(dds, parallel = FALSE)
res <- DESeq2::results(dds, contrast = c("dex", "trt", "untrt"), alpha = 0.05)
res <- res[order(res$padj, na.last = TRUE), ]
deg_results <- as.data.frame(res)
deg_results$gene_id <- rownames(deg_results)
deg_results$regulation <- "Not significant"
deg_results$regulation[!is.na(deg_results$padj) & deg_results$padj < 0.05 & deg_results$log2FoldChange >= 1] <- "Up"
deg_results$regulation[!is.na(deg_results$padj) & deg_results$padj < 0.05 & deg_results$log2FoldChange <= -1] <- "Down"
deg_results$regulation <- factor(deg_results$regulation, levels = c("Down", "Not significant", "Up"))
write.csv(deg_results, file.path(OUTPUT_DIR, "DESeq2_DEG_results.csv"), row.names = FALSE)
cat("DESeq2: Up =", sum(deg_results$regulation == "Up"),
    ", Down =", sum(deg_results$regulation == "Down"), "\n")

# ------------------------ 3. VST and PCA ------------------------
vsd <- DESeq2::vst(dds, blind = FALSE)
vst_matrix <- SummarizedExperiment::assay(vsd)
pca <- DESeq2::plotPCA(vsd, intgroup = c("dex", "cell"), returnData = TRUE)
pca_var <- round(100 * attr(pca, "percentVar"), 1)
pca_plot <- ggplot(pca, aes(x = PC1, y = PC2, color = dex, shape = cell)) +
  geom_point(size = 3.8) +
  geom_text(aes(label = name), vjust = -0.9, show.legend = FALSE, size = 3) +
  scale_color_manual(values = c(untrt = "#2875B9", trt = "#D64B4B")) +
  labs(title = "Airway RNA-seq | PCA", subtitle = "Dexamethasone treatment; donor-controlled DESeq2",
       x = paste0("PC1: ", pca_var[1], "% variance"),
       y = paste0("PC2: ", pca_var[2], "% variance"), color = "Treatment", shape = "Cell line") +
  theme_bw(base_size = 11)

# ------------------------ 4. MA and volcano ------------------------
ma_data <- deg_results[is.finite(deg_results$baseMean) & deg_results$baseMean > 0 &
                         is.finite(deg_results$log2FoldChange), ]
ma_plot <- ggplot(ma_data, aes(baseMean, log2FoldChange, color = regulation)) +
  geom_point(size = 0.9, alpha = 0.55) +
  scale_x_log10() +
  scale_color_manual(values = c(Down = "#276FBF", `Not significant` = "#A8ABB1", Up = "#D94848")) +
  geom_hline(yintercept = 0, linewidth = 0.4) +
  labs(title = "DESeq2 MA plot", x = "Mean normalized count", y = "log2 fold change") +
  theme_bw(base_size = 11)

volcano_data <- deg_results[is.finite(deg_results$log2FoldChange) &
                              !is.na(deg_results$padj), ]
volcano_data$neglog10padj <- -log10(pmax(volcano_data$padj, .Machine$double.xmin))
volcano_plot <- ggplot(volcano_data,
                       aes(log2FoldChange, neglog10padj, color = regulation)) +
  geom_point(size = 1.15, alpha = 0.65) +
  geom_vline(xintercept = c(-1, 1), linetype = "dashed", color = "gray55") +
  geom_hline(yintercept = -log10(0.05), linetype = "dashed", color = "gray55") +
  scale_color_manual(values = c(Down = "#276FBF", `Not significant` = "#A8ABB1", Up = "#D94848")) +
  labs(title = "Airway RNA-seq | Volcano", x = "log2 fold change (trt vs untrt)",
       y = "-log10 adjusted p-value", color = "DEG status") +
  theme_bw(base_size = 11)

# ------------------------ 5. Heatmap ------------------------
valid_padj <- which(!is.na(res$padj))
if (length(valid_padj) < 2) stop("Not enough genes with adjusted p-values for heatmap")
top_ids <- rownames(res)[head(valid_padj, 30)]
heatmap_matrix <- vst_matrix[top_ids, , drop = FALSE]
heatmap_matrix <- t(scale(t(heatmap_matrix)))
heatmap_matrix[!is.finite(heatmap_matrix)] <- 0
annotation_col <- data.frame(Treatment = sample_info$dex, Cell = sample_info$cell,
                             row.names = rownames(sample_info))
heatmap_plot <- pheatmap::pheatmap(
  heatmap_matrix,
  annotation_col = annotation_col,
  cluster_rows = TRUE, cluster_cols = TRUE,
  show_rownames = TRUE, fontsize_row = 6,
  main = "Top 30 DEGs | row-scaled VST",
  color = grDevices::colorRampPalette(c("#2166AC", "white", "#B2182B"))(100),
  silent = TRUE
)

# ------------------------ 6. Hallmark GSEA (optional) ------------------------
# Uses Ensembl IDs directly to avoid ambiguous Ensembl-to-symbol mapping.
# msigdbr can require an internet connection on its initial download.
gsea_dot_plot <- NULL
gsea_results <- NULL
if (RUN_GSEA) {
  gsea_required <- c("fgsea", "msigdbr")
  missing_gsea <- gsea_required[!vapply(gsea_required, requireNamespace, logical(1), quietly = TRUE)]
  if (length(missing_gsea)) {
    warning("GSEA skipped: missing ", paste(missing_gsea, collapse = ", "))
  } else {
    tryCatch({
      hallmark <- msigdbr::msigdbr(species = "Homo sapiens", collection = "H")
      hallmark <- hallmark[!is.na(hallmark$ensembl_gene) & nzchar(hallmark$ensembl_gene), ]
      pathways <- split(hallmark$ensembl_gene, hallmark$gs_name)
      pathways <- lapply(pathways, unique)
      gene_ranks <- res$stat
      names(gene_ranks) <- rownames(res)
      # Match identifier forms (airway row names may include Ensembl version suffixes).
      names(gene_ranks) <- sub("\\.[0-9]+$", "", names(gene_ranks))
      gene_ranks <- gene_ranks[is.finite(gene_ranks)]
      gene_ranks <- sort(gene_ranks, decreasing = TRUE)
      gene_ranks <- gene_ranks[!duplicated(names(gene_ranks))]
      set.seed(2026)
      fgsea_res <- fgsea::fgseaMultilevel(
        pathways = pathways, stats = gene_ranks, minSize = 15, maxSize = 500,
        BPPARAM = BiocParallel::SerialParam()
      )
      gsea_results <- as.data.frame(fgsea_res)
      gsea_results$leadingEdge <- vapply(gsea_results$leadingEdge,
                                         function(x) paste(x, collapse = ";"), character(1))
      gsea_results <- gsea_results[order(gsea_results$padj, na.last = TRUE), ]
      write.csv(gsea_results, file.path(OUTPUT_DIR, "Hallmark_GSEA_results.csv"), row.names = FALSE)
      plot_rows <- gsea_results[is.finite(gsea_results$NES) & !is.na(gsea_results$padj), ]
      plot_rows <- head(plot_rows, 18)
      if (nrow(plot_rows)) {
        plot_rows$pathway_short <- gsub("^HALLMARK_", "", plot_rows$pathway)
        plot_rows$pathway_short <- gsub("_", " ", plot_rows$pathway_short)
        plot_rows$pathway_short <- factor(plot_rows$pathway_short,
                                          levels = rev(plot_rows$pathway_short))
        gsea_dot_plot <- ggplot(plot_rows,
                               aes(x = NES, y = pathway_short, size = size, color = NES)) +
          geom_point() +
          scale_color_gradient2(low = "blue", mid = "white", high = "red", midpoint = 0,
                                name = "Normalized\nEnrichment\nScore (NES)") +
          scale_size_continuous(name = "Gene count", range = c(2.5, 8)) +
          labs(title = "Hallmark GSEA | airway", x = "Normalized Enrichment Score", y = NULL) +
          theme_bw(base_size = 11) +
          theme(panel.grid.major.y = element_blank())
      } else {
        message("GSEA returned no finite NES values to plot.")
      }
    }, error = function(e) warning("GSEA skipped: ", conditionMessage(e)))
  }
}

# ------------------------ 7. Export graphics ------------------------
# Plot Registry named objects:
# pca_plot, ma_plot, volcano_plot, heatmap_plot, and (if successful) gsea_dot_plot.
# Do not overwrite these objects with the result of ggsave().
export_gg <- function(plot, filename, width = 8, height = 5.5) {
  ggplot2::ggsave(file.path(OUTPUT_DIR, paste0(filename, ".png")),
                  plot = plot, width = width, height = height, dpi = 180, bg = "white")
  if (requireNamespace("svglite", quietly = TRUE)) {
    ggplot2::ggsave(file.path(OUTPUT_DIR, paste0(filename, ".svg")),
                    plot = plot, width = width, height = height,
                    device = svglite::svglite, bg = "white")
  } else {
    message("SVG export skipped for ", filename, ": install svglite (Plot Registry may use R SVG fallback).")
  }
}
export_gg(pca_plot, "PCA")
export_gg(ma_plot, "MA")
export_gg(volcano_plot, "Volcano")
if (!is.null(gsea_dot_plot)) export_gg(gsea_dot_plot, "Hallmark_GSEA_dotplot", 9, 7)

pheatmap::pheatmap(heatmap_matrix, annotation_col = annotation_col,
                   show_rownames = TRUE, fontsize_row = 6,
                   main = "Top 30 DEGs | row-scaled VST",
                   color = grDevices::colorRampPalette(c("#2166AC", "white", "#B2182B"))(100),
                   filename = file.path(OUTPUT_DIR, "Heatmap.png"), width = 8, height = 7)

# ------------------------ 8. Report ------------------------
cat("\n=== AIRWAY RNA-seq ANALYSIS FINISHED ===\n")
cat("Output directory:", normalizePath(OUTPUT_DIR), "\n")
cat("Genes passing prefilter:", nrow(dds), "\n")
cat("DEG: Up", sum(deg_results$regulation == "Up"),
    "| Down", sum(deg_results$regulation == "Down"), "\n")
cat("Registry plots: pca_plot, ma_plot, volcano_plot, heatmap_plot",
    if (!is.null(gsea_dot_plot)) ", gsea_dot_plot" else " (GSEA plot unavailable)", "\n")
print(sessionInfo())
