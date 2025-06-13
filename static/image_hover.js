// static/image_hover.js

document.addEventListener('DOMContentLoaded', () => {
    let hoverPreview = null;
    let previewTimeout = null;
    
    // Create the hover preview element
    function createHoverPreview() {
        const preview = document.createElement('div');
        preview.className = 'image-hover-preview';
        preview.innerHTML = `
            <img src="" alt="">
            <div class="image-name"></div>
        `;
        document.body.appendChild(preview);
        return preview;
    }
    
    // Initialize hover preview element
    function initHoverPreview() {
        if (!hoverPreview) {
            hoverPreview = createHoverPreview();
        }
    }
    
    // Show hover preview
    function showHoverPreview(element, imageUrl, imageName, mouseEvent) {
        initHoverPreview();
        
        const img = hoverPreview.querySelector('img');
        const nameDiv = hoverPreview.querySelector('.image-name');
        
        // Set image source and name
        img.src = imageUrl;
        img.alt = imageName;
        nameDiv.textContent = imageName;
        
        // Position the preview
        positionHoverPreview(mouseEvent);
        
        // Show preview with delay
        clearTimeout(previewTimeout);
        previewTimeout = setTimeout(() => {
            hoverPreview.classList.add('visible');
        }, 300); // 300ms delay before showing
    }
    
    // Hide hover preview
    function hideHoverPreview() {
        clearTimeout(previewTimeout);
        if (hoverPreview) {
            hoverPreview.classList.remove('visible');
        }
    }
    
    // Position the hover preview near the mouse
    function positionHoverPreview(mouseEvent) {
        if (!hoverPreview) return;
        
        const preview = hoverPreview;
        const margin = 15; // Distance from cursor
        
        // Get viewport dimensions
        const viewportWidth = window.innerWidth;
        const viewportHeight = window.innerHeight;
        
        // Set initial position to get dimensions
        preview.style.left = '0px';
        preview.style.top = '0px';
        
        // Get preview dimensions
        const previewRect = preview.getBoundingClientRect();
        const previewWidth = previewRect.width;
        const previewHeight = previewRect.height;
        
        // Calculate position
        let left = mouseEvent.clientX + margin;
        let top = mouseEvent.clientY + margin;
        
        // Adjust if would go off right edge
        if (left + previewWidth > viewportWidth) {
            left = mouseEvent.clientX - previewWidth - margin;
        }
        
        // Adjust if would go off bottom edge
        if (top + previewHeight > viewportHeight) {
            top = mouseEvent.clientY - previewHeight - margin;
        }
        
        // Ensure it doesn't go off left or top edges
        left = Math.max(margin, left);
        top = Math.max(margin, top);
        
        // Apply position with scroll offset
        preview.style.left = (left + window.scrollX) + 'px';
        preview.style.top = (top + window.scrollY) + 'px';
    }
    
    // Update position as mouse moves
    function updateHoverPreviewPosition(mouseEvent) {
        if (hoverPreview && hoverPreview.classList.contains('visible')) {
            positionHoverPreview(mouseEvent);
        }
    }
    
    // Event delegation for image file hover
    document.addEventListener('mouseover', (event) => {
        const imageItem = event.target.closest('li[data-image-url]');
        if (imageItem) {
            const imageUrl = imageItem.getAttribute('data-image-url');
            const imageName = imageItem.querySelector('span:not(.icon)')?.textContent || 'Image';
            
            if (imageUrl) {
                showHoverPreview(imageItem, imageUrl, imageName, event);
            }
        }
    });
    
    // Hide preview when mouse leaves image item
    document.addEventListener('mouseout', (event) => {
        const imageItem = event.target.closest('li[data-image-url]');
        if (imageItem) {
            // Check if we're really leaving the item (not just moving to a child)
            if (!imageItem.contains(event.relatedTarget)) {
                hideHoverPreview();
            }
        }
    });
    
    // Update position as mouse moves over image items
    document.addEventListener('mousemove', (event) => {
        const imageItem = event.target.closest('li[data-image-url]');
        if (imageItem) {
            updateHoverPreviewPosition(event);
        }
    });
    
    // Hide preview on scroll
    window.addEventListener('scroll', hideHoverPreview);
    
    // Hide preview when HTMX swaps content
    htmx.on('htmx:beforeSwap', hideHoverPreview);
    
    // Clean up preview on page unload
    window.addEventListener('beforeunload', () => {
        clearTimeout(previewTimeout);
        if (hoverPreview) {
            hoverPreview.remove();
        }
    });
});