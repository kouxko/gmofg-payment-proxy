/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20231010 	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

public class ConfirmData extends BaseData{

    private ConfirmResult result;

    @Override
    public ConfirmResult getBaseResult() {
        return result;
    }

    public void setResult(ConfirmResult confirmResult) {
        result = confirmResult;
    }

    public static class ConfirmResult extends BaseResult {

        private String payType;
        private String payCheckDate;

        public void setPayType(String payType) {
            this.payType = payType;
        }
        public String getPayType() {
            return payType;
        }


        public String getPayCheckDate() {
            return payCheckDate;
        }
        public void setPayCheckDate(String payCheckDate) {
            this.payCheckDate = payCheckDate;
        }
    }
}
