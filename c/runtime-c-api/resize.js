/*
 @licstart  The following is the entire license notice for the
 JavaScript code in this file.

 Copyright (C) 1997-2017 by Dimitri van Heesch

 This program is free software; you can redistribute it and/or modify
 it under the terms of the GNU General Public License as published by
 the Free Software Foundation; either version 2 of the License, or
 (at your option) any later version.

 This program is distributed in the hope that it will be useful,
 but WITHOUT ANY WARRANTY; without even the implied warranty of
 MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 GNU General Public License for more details.

 You should have received a copy of the GNU General Public License along
 with this program; if not, write to the Free Software Foundation, Inc.,
 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.

 @licend  The above is the entire license notice
 for the JavaScript code in this file
 */
function initResizable()
{
  var cookie_namespace = 'doxygen';
  var sidenav,navtree,content,header,collapsed,collapsedWidth=0,barWidth=6,desktop_vp=768,titleHeight;

  function readCookie(cookie)
  {
    var myCookie = cookie_namespace+"_"+cookie+"=";
    if (document.cookie) {
      var index = document.cookie.indexOf(myCookie);
      if (index != -1) {
        var valStart = index + myCookie.length;
        var valEnd = document.cookie.indexOf(";", valStart);
        if (valEnd == -1) {
          valEnd = document.cookie.length;
        }
        var val = document.cookie.substring(valStart, valEnd);
        return val;
      }
    }
    return 0;
  }

  function writeCookie(cookie, val, expiration)
  {
    if (val==undefined) return;
    if (expiration == null) {
      var date = new Date();
      date.setTime(date.getTime()+(10*365*24*60*60*1000)); // default expiration is one week
      expiration = date.toGMTString();
    }
    document.cookie = cookie_namespace + "_" + cookie + "=" + val + "; expires=" + expiration+"; path=/";
  }

  function resizeWidth()
  {
    var windowWidth = $(window).width() + "px";
    var sidenavWidth = $(sidenav).outerWidth();
    content.css({marginLeft:parseInt(sidenavWidth)+"px"});
    writeCookie('width',sidenavWidth-barWidth, null);
  }

  function restoreWidth(navWidth)
  {
    var windowWidth = $(window).width() + "px";
    content.css({marginLeft:parseInt(navWidth)+barWidth+"px"});
    sidenav.css({width:navWidth + "px"});
  }

  function resizeHeight()
  {
    var headerHeight = header.outerHeight();
    var footerHeight = footer.outerHeight();
    var windowHeight = $(window).height() - headerHeight - footerHeight;
    content.css({height:windowHeight + "px"});
    navtree.css({height:windowHeight + "px"});
    sidenav.css({height:windowHeight + "px"});
    var width=$(window).width();
    if (width!=collapsedWidth) {
      if (width<desktop_vp && collapsedWidth>=desktop_vp) {
        if (!collapsed) {
          collapseExpand();
        }
      } else if (width>desktop_vp && collapsedWidth<desktop_vp) {
        if (collapsed) {
          collapseExpand();
        }
      }
      collapsedWidth=width;
    }
    (document.getElementById(location.hash.slice(1))||document.body).scrollIntoView();
  }

  function collapseExpand()
  {
    if (sidenav.width()>0) {
      restoreWidth(0);
      collapsed=true;
    }
    else {
      var width = readCookie('width');
      if (width>200 && width<$(window).width()) { restoreWidth(width); } else { restoreWidth(200); }
      collapsed=false;
    }
  }

  header  = $("#top");
  sidenav = $("#side-nav");
  content = $("#doc-content");
  navtree = $("#nav-tree");
  footer  = $("#nav-path");
  $(".side-nav-resizable").resizable({resize: function(e, ui) { resizeWidth(); } });
  $(sidenav).resizable({ minWidth: 0 });
  $(window).resize(function() { resizeHeight(); });
  var device = navigator.userAgent.toLowerCase();
  var touch_device = device.match(/(iphone|ipod|ipad|android)/);
  if (touch_device) { /* wider split bar for touch only devices */
    $(sidenav).css({ paddingRight:'20px' });
    $('.ui-resizable-e').css({ width:'20px' });
    $('#nav-sync').css({ right:'34px' });
    barWidth=20;
  }
  var width = readCookie('width');
  if (width) { restoreWidth(width); } else { resizeWidth(); }
  resizeHeight();
  var url = location.href;
  var i=url.indexOf("#");
  if (i>=0) window.location.hash=url.substr(i);
  var _preventDefault = function(evt) { evt.preventDefault(); };
  $("#splitbar").bind("dragstart", _preventDefault).bind("selectstart", _preventDefault);
  $(".ui-resizable-handle").dblclick(collapseExpand);
  $(window).on('load',resizeHeight);
}
/* @license-end */
